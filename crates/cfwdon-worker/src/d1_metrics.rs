use std::cell::{Cell, RefCell};

use worker::{Response, Result};

/// Per-query duration that triggers a structured `d1_slow_query` log line.
pub(crate) const D1_SLOW_QUERY_MS: u64 = 500;
/// Previous-request D1 SQL budget that triggers 503 load-shedding on heavy public reads.
pub(crate) const D1_LOAD_SHED_SQL_MS: u64 = 8_000;
const D1_LOAD_SHED_RETRY_AFTER_SECS: u32 = 5;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct D1RequestMetrics {
    pub query_count: u32,
    pub sql_ms_sum: u64,
}

thread_local! {
    static D1_REQUEST_METRICS: RefCell<D1RequestMetrics> =
        RefCell::new(D1RequestMetrics::default());
    static LAST_REQUEST_D1_SQL_MS: Cell<u64> = const { Cell::new(0) };
}

pub(crate) fn reset_d1_request_metrics() {
    D1_REQUEST_METRICS.with(|metrics| *metrics.borrow_mut() = D1RequestMetrics::default());
}

pub(crate) fn snapshot_d1_request_metrics() -> D1RequestMetrics {
    D1_REQUEST_METRICS.with(|metrics| *metrics.borrow())
}

pub(crate) fn record_d1_query_duration(duration_ms: u64) {
    D1_REQUEST_METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        metrics.query_count = metrics.query_count.saturating_add(1);
        metrics.sql_ms_sum = metrics.sql_ms_sum.saturating_add(duration_ms);
    });
    maybe_log_slow_d1_query(duration_ms);
}

pub(crate) fn record_d1_wall_clock(started_at_ms: f64) {
    let duration_ms = (js_sys::Date::now() - started_at_ms).max(0.0).round() as u64;
    record_d1_query_duration(duration_ms);
}

fn maybe_log_slow_d1_query(duration_ms: u64) {
    if duration_ms < D1_SLOW_QUERY_MS {
        return;
    }
    // console logging is wasm-only; native unit tests record metrics without JS.
    #[cfg(target_arch = "wasm32")]
    crate::log_json_event(crate::add_log_message(
        serde_json::json!({
            "event": "d1_slow_query",
            "duration_ms": duration_ms,
            "threshold_ms": D1_SLOW_QUERY_MS,
        }),
        format!("D1 query exceeded {D1_SLOW_QUERY_MS}ms threshold ({duration_ms}ms)"),
    ));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = duration_ms;
}

/// Remember this request's D1 SQL cost so the next request in the same isolate
/// can fail-fast under sustained D1 pressure instead of hanging until cancellation.
pub(crate) fn publish_d1_request_pressure() {
    let metrics = snapshot_d1_request_metrics();
    LAST_REQUEST_D1_SQL_MS.with(|slot| slot.set(metrics.sql_ms_sum));
}

pub(crate) fn last_request_d1_sql_ms() -> u64 {
    LAST_REQUEST_D1_SQL_MS.with(|slot| slot.get())
}

pub(crate) fn d1_pressure_load_shed_response() -> Result<Option<Response>> {
    let previous_sql_ms = last_request_d1_sql_ms();
    if previous_sql_ms < D1_LOAD_SHED_SQL_MS {
        return Ok(None);
    }

    let mut response = Response::from_json(&serde_json::json!({
        "error": "Service temporarily unavailable",
        "error_description": "D1 latency pressure; retry shortly",
    }))?
    .with_status(503);
    response
        .headers_mut()
        .set("Retry-After", &D1_LOAD_SHED_RETRY_AFTER_SECS.to_string())?;
    Ok(Some(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_metrics_accumulate_and_reset() {
        reset_d1_request_metrics();
        record_d1_query_duration(12);
        record_d1_query_duration(8);
        assert_eq!(
            snapshot_d1_request_metrics(),
            D1RequestMetrics {
                query_count: 2,
                sql_ms_sum: 20,
            }
        );
        reset_d1_request_metrics();
        assert_eq!(snapshot_d1_request_metrics(), D1RequestMetrics::default());
    }

    #[test]
    fn publish_d1_request_pressure_tracks_previous_request() {
        reset_d1_request_metrics();
        LAST_REQUEST_D1_SQL_MS.with(|slot| slot.set(0));
        record_d1_query_duration(D1_LOAD_SHED_SQL_MS);
        publish_d1_request_pressure();
        assert_eq!(last_request_d1_sql_ms(), D1_LOAD_SHED_SQL_MS);
        reset_d1_request_metrics();
        LAST_REQUEST_D1_SQL_MS.with(|slot| slot.set(0));
    }
}
