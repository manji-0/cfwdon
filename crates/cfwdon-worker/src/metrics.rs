//! Optional Workers Analytics Engine data points.
//!
//! The dataset binding is [`METRICS_BINDING`] (`[[analytics_engine_datasets]]`
//! in `wrangler.toml`). A deployment without the binding gets a no-op: no
//! point is built and no error is logged. Writes never fail into a request,
//! queue or cron path.
//!
//! Cloudflare drops data points past 250 per Worker invocation without an
//! error, so each invocation keeps a budget of [`POINTS_PER_INVOCATION`] - 1
//! writes and counts what it left out. [`finish_invocation`] spends the last
//! slot on one `metrics.dropped` point, so a busy invocation says it was cut
//! short. The budget lives in thread-local state like `d1_metrics`, so
//! concurrent requests in one isolate share the budget of the latest
//! [`begin_invocation`]; that only makes the cap approximate.
//!
//! Point conventions (index is always one fixed string, doubles and blobs are
//! positional and low-cardinality, no PII):
//!
//! | index | doubles | blobs |
//! | --- | --- | --- |
//! | `http.request` | status, latency ms, D1 queries, D1 SQL ms | route class, method, status class |
//! | `federation.event` | none | event, outcome |
//! | `cron.task` | duration ms | task, `ok` or `error` |
//! | `queue.batch` | messages, duration ms | none |
//! | `metrics.dropped` | dropped, written | source, index that lost most |

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use worker::{AnalyticsEngineDataPointBuilder, AnalyticsEngineDataset, Env, console_warn};

pub(crate) const METRICS_BINDING: &str = "METRICS";

/// Analytics Engine's documented per-invocation cap on data points.
pub(crate) const POINTS_PER_INVOCATION: u32 = 250;

const BLOB_MAX_CHARS: usize = 128;

#[derive(Debug, Default)]
struct Budget {
    written: u32,
    dropped: BTreeMap<&'static str, u32>,
}

impl Budget {
    /// Take one write slot, keeping the last one for the `metrics.dropped` summary.
    fn try_take(&mut self, index: &'static str) -> bool {
        if self.written + 1 >= POINTS_PER_INVOCATION {
            *self.dropped.entry(index).or_insert(0) += 1;
            return false;
        }
        self.written += 1;
        true
    }

    /// `(dropped, written, index that lost the most)`; `None` when nothing was dropped.
    fn dropped_summary(&self) -> Option<(u32, u32, &'static str)> {
        let total: u32 = self.dropped.values().sum();
        if total == 0 {
            return None;
        }
        let top = self
            .dropped
            .iter()
            .fold(("", 0), |top, (index, count)| {
                if *count > top.1 {
                    (*index, *count)
                } else {
                    top
                }
            })
            .0;
        Some((total, self.written, top))
    }
}

thread_local! {
    static DATASET: RefCell<Option<AnalyticsEngineDataset>> = const { RefCell::new(None) };
    static BUDGET: RefCell<Budget> = RefCell::new(Budget::default());
}

/// Start an invocation: look up the dataset binding and reset the budget.
pub(crate) fn begin_invocation(env: &Env) {
    DATASET.with(|slot| *slot.borrow_mut() = env.analytics_engine(METRICS_BINDING).ok());
    BUDGET.with(|budget| *budget.borrow_mut() = Budget::default());
}

/// End an invocation: report points left out for lack of budget, if any.
pub(crate) fn finish_invocation(source: &str) {
    let Some((dropped, written, top)) = BUDGET.with(|budget| budget.borrow().dropped_summary())
    else {
        return;
    };
    write_point(
        "metrics.dropped",
        &[f64::from(dropped), f64::from(written)],
        &[source, top],
    );
    console_warn!(
        "{}",
        serde_json::json!({
            "source": "cfwdon",
            "event": "metrics_dropped",
            "invocation": source,
            "dropped": dropped,
            "written": written,
            "top_index": top,
        })
    );
}

/// Best-effort data point under the invocation budget.
pub(crate) fn write_metric(index: &'static str, doubles: &[f64], blobs: &[&str]) {
    if !DATASET.with(|slot| slot.borrow().is_some()) {
        return;
    }
    if !BUDGET.with(|budget| budget.borrow_mut().try_take(index)) {
        return;
    }
    write_point(index, doubles, blobs);
}

fn write_point(index: &str, doubles: &[f64], blobs: &[&str]) {
    DATASET.with(|slot| {
        let Some(dataset) = slot.borrow().clone() else {
            return;
        };
        let blobs = blobs.iter().map(|blob| truncate_blob(blob));
        let _ = AnalyticsEngineDataPointBuilder::new()
            .indexes([index])
            .doubles(doubles.iter().copied())
            .blobs(blobs)
            .write_to(&dataset);
    });
}

fn truncate_blob(blob: &str) -> String {
    blob.chars().take(BLOB_MAX_CHARS).collect()
}

/// Coarse route class for the `http.request` point. Unknown first segments
/// collapse to `other` so scanners cannot grow the label set.
pub(crate) fn route_class(path: &str) -> String {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    match segments.next() {
        None => "root".to_owned(),
        Some("api") => {
            let version = segments.next().unwrap_or("");
            let area = segments.next().unwrap_or("");
            if area.is_empty() || !area.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                format!("api/{version}")
            } else {
                format!("api/{version}/{area}")
            }
        }
        Some(".well-known") => match segments.next() {
            Some(
                name @ ("webfinger"
                | "nodeinfo"
                | "host-meta"
                | "host-meta.json"
                | "oauth-authorization-server"),
            ) => format!(".well-known/{name}"),
            _ => ".well-known".to_owned(),
        },
        Some(
            first @ ("oauth"
            | "users"
            | "inbox"
            | "media"
            | "nodeinfo"
            | "app"
            | "admin"
            | "profiles"
            | "authorize_interaction"
            | "share"
            | "assets"),
        ) => first.to_owned(),
        Some(first) if first.starts_with('@') => "profile".to_owned(),
        Some(_) => "other".to_owned(),
    }
}

fn status_class(status: u16) -> &'static str {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        _ => "5xx",
    }
}

/// One finished HTTP request.
pub(crate) fn write_http_request_metric(
    method: &str,
    path: &str,
    status: u16,
    duration_ms: u64,
    d1_query_count: u32,
    d1_sql_ms: u64,
) {
    write_metric(
        "http.request",
        &[
            f64::from(status),
            duration_ms as f64,
            f64::from(d1_query_count),
            d1_sql_ms as f64,
        ],
        &[&route_class(path), method, status_class(status)],
    );
}

/// One federation lifecycle event (delivery, inbox, queue kick).
pub(crate) fn write_federation_event_metric(event: &str, outcome: &str) {
    write_metric("federation.event", &[], &[event, outcome]);
}

/// One queue batch.
///
/// The consumer acks or retries the batch itself and returns `Ok` either way,
/// so failures show up as `federation.event` points
/// (`outbox_queue_processing_failed`), not here.
pub(crate) fn write_queue_batch_metric(messages: usize, duration_ms: u64) {
    write_metric("queue.batch", &[messages as f64, duration_ms as f64], &[]);
}

/// Run one cron step and record its duration and outcome as a `cron.task` point.
pub(crate) async fn timed_cron_task<T, E>(
    task: &'static str,
    run: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let started_at = js_sys::Date::now();
    let result = run.await;
    let duration_ms = (js_sys::Date::now() - started_at).max(0.0).round();
    write_metric(
        "cron.task",
        &[duration_ms],
        &[task, if result.is_ok() { "ok" } else { "error" }],
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_keeps_one_slot_for_the_dropped_summary() {
        let mut budget = Budget::default();
        for _ in 0..POINTS_PER_INVOCATION - 1 {
            assert!(budget.try_take("http.request"));
        }
        assert!(!budget.try_take("federation.event"));
        assert!(!budget.try_take("federation.event"));
        assert!(!budget.try_take("http.request"));
        assert_eq!(
            budget.dropped_summary(),
            Some((3, POINTS_PER_INVOCATION - 1, "federation.event"))
        );
    }

    #[test]
    fn budget_under_the_cap_reports_nothing() {
        let mut budget = Budget::default();
        assert!(budget.try_take("cron.task"));
        assert_eq!(budget.dropped_summary(), None);
    }

    #[test]
    fn route_class_is_bounded() {
        assert_eq!(route_class("/"), "root");
        assert_eq!(
            route_class("/api/v1/timelines/tag/rust"),
            "api/v1/timelines"
        );
        assert_eq!(
            route_class("/api/v1/statuses/018f1a2b3c4d5e6f7a8b9c0d"),
            "api/v1/statuses"
        );
        assert_eq!(route_class("/api/v2/search"), "api/v2/search");
        assert_eq!(route_class("/api/v1/018f1a2b"), "api/v1");
        assert_eq!(
            route_class("/.well-known/webfinger"),
            ".well-known/webfinger"
        );
        assert_eq!(route_class("/.well-known/random"), ".well-known");
        assert_eq!(route_class("/users/alice/inbox"), "users");
        assert_eq!(route_class("/@alice"), "profile");
        assert_eq!(route_class("/wp-login.php"), "other");
    }

    #[test]
    fn status_class_buckets() {
        assert_eq!(status_class(200), "2xx");
        assert_eq!(status_class(404), "4xx");
        assert_eq!(status_class(503), "5xx");
    }

    #[test]
    fn blobs_are_truncated() {
        assert_eq!(
            truncate_blob(&"a".repeat(500)).chars().count(),
            BLOB_MAX_CHARS
        );
    }
}
