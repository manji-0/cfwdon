use serde::{Deserialize, Serialize};
use worker::{
    DurableObject, Env, Method, Request, RequestInit, Response, Result, State, console_error,
    durable_object,
};

const INBOX_HOST_ADMIT_PATH: &str = "/admit";
const INBOX_HOST_INTERNAL_ORIGIN: &str = "https://inbox-host";

/// Fixed-window admit window length (seconds).
pub(crate) const INBOX_HOST_ADMIT_WINDOW_SECS: u64 = 60;
/// Default max admits per window per remote host.
pub(crate) const INBOX_HOST_ADMIT_LIMIT: u64 = 120;

const STORAGE_WINDOW_START_KEY: &str = "admit_window_start_ms";
const STORAGE_COUNT_KEY: &str = "admit_count";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct InboxHostAdmitRequest {
    activity_id: Option<String>,
    actor_uri: Option<String>,
    activity_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct InboxHostAdmitResponse {
    allowed: bool,
    count: u64,
    retry_after_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InboxHostAdmitResult {
    pub allowed: bool,
    pub count: u32,
    pub retry_after_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FixedWindowAdmission {
    pub window_start_ms: u64,
    pub count: u64,
    pub allowed: bool,
}

#[durable_object]
pub struct InboxHost {
    state: State,
    #[allow(dead_code)]
    env: Env,
}

impl DurableObject for InboxHost {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let path = req.path();
        if req.method() == Method::Post && path.ends_with(INBOX_HOST_ADMIT_PATH) {
            return self.handle_admit(req).await;
        }

        Response::error("Not found", 404)
    }
}

impl InboxHost {
    async fn handle_admit(&self, mut req: Request) -> Result<Response> {
        let _admit = req.json::<InboxHostAdmitRequest>().await?;
        let now_ms = inbox_host_now_millis();
        let window_ms = INBOX_HOST_ADMIT_WINDOW_SECS * 1000;

        let storage = self.state.storage();
        let window_start_ms = storage
            .get::<u64>(STORAGE_WINDOW_START_KEY)
            .await?
            .unwrap_or(now_ms);
        let count = storage.get::<u64>(STORAGE_COUNT_KEY).await?.unwrap_or(0);

        let admission = fixed_window_admission(
            Some(window_start_ms),
            count,
            now_ms,
            window_ms,
            INBOX_HOST_ADMIT_LIMIT,
        );

        if admission.allowed {
            storage
                .put(STORAGE_WINDOW_START_KEY, admission.window_start_ms)
                .await?;
            storage.put(STORAGE_COUNT_KEY, admission.count).await?;
        }

        let retry_after_secs = if admission.allowed {
            0
        } else {
            fixed_window_retry_after_secs(admission.window_start_ms, now_ms, window_ms)
        };

        Response::from_json(&InboxHostAdmitResponse {
            allowed: admission.allowed,
            count: admission.count,
            retry_after_secs,
        })
    }
}

pub(crate) fn inbox_host_id_name(host_key: &str) -> String {
    format!("inbox-host:{host_key}")
}

pub(crate) fn fixed_window_admission(
    window_start_ms: Option<u64>,
    count: u64,
    now_ms: u64,
    window_ms: u64,
    limit: u64,
) -> FixedWindowAdmission {
    let start = window_start_ms.unwrap_or(now_ms);
    let elapsed = now_ms.saturating_sub(start);

    if window_start_ms.is_none() || elapsed >= window_ms {
        return FixedWindowAdmission {
            window_start_ms: now_ms,
            count: 1,
            allowed: true,
        };
    }

    if count >= limit {
        return FixedWindowAdmission {
            window_start_ms: start,
            count,
            allowed: false,
        };
    }

    FixedWindowAdmission {
        window_start_ms: start,
        count: count + 1,
        allowed: true,
    }
}

fn inbox_host_now_millis() -> u64 {
    js_sys::Date::now() as u64
}

/// Seconds until the fixed window resets; at least 1 when the caller is denied.
pub(crate) fn fixed_window_retry_after_secs(
    window_start_ms: u64,
    now_ms: u64,
    window_ms: u64,
) -> u64 {
    let window_end_ms = window_start_ms.saturating_add(window_ms);
    if now_ms >= window_end_ms {
        return 1;
    }
    let remaining_ms = window_end_ms - now_ms;
    ((remaining_ms + 999) / 1000).max(1)
}

fn inbox_host_admit_result_from_response(admit: InboxHostAdmitResponse) -> InboxHostAdmitResult {
    InboxHostAdmitResult {
        allowed: admit.allowed,
        count: u32::try_from(admit.count).unwrap_or(u32::MAX),
        retry_after_secs: admit.retry_after_secs,
    }
}

fn inbox_host_admit_allowed_open() -> InboxHostAdmitResult {
    InboxHostAdmitResult {
        allowed: true,
        count: 0,
        retry_after_secs: 0,
    }
}

pub(crate) async fn admit_inbox_host(
    env: &Env,
    binding: &str,
    host_key: &str,
    activity_id: &str,
    actor_uri: &str,
    activity_type: &str,
) -> Result<InboxHostAdmitResult> {
    let namespace = env.durable_object(binding)?;
    let object_name = inbox_host_id_name(host_key);
    let stub = namespace.get_by_name(&object_name)?;

    let body = serde_json::json!({
        "activity_id": activity_id,
        "actor_uri": actor_uri,
        "activity_type": activity_type,
    });
    let body_json = serde_json::to_string(&body).map_err(|error| {
        worker::Error::RustError(format!("failed to encode inbox host admit body: {error}"))
    })?;

    let headers = worker::Headers::new();
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body_json)));

    let url = format!("{INBOX_HOST_INTERNAL_ORIGIN}{INBOX_HOST_ADMIT_PATH}");
    let request = Request::new_with_init(&url, &init)?;
    let mut response = stub.fetch_with_request(request).await?;
    if response.status_code() >= 400 {
        return Err(worker::Error::RustError(format!(
            "inbox host admit failed with HTTP {}",
            response.status_code()
        )));
    }

    let admit = response.json::<InboxHostAdmitResponse>().await?;
    Ok(inbox_host_admit_result_from_response(admit))
}

pub(crate) async fn admit_inbox_host_soft(
    env: Option<&Env>,
    binding: &str,
    host_key: &str,
    activity_id: &str,
    actor_uri: &str,
    activity_type: &str,
) -> InboxHostAdmitResult {
    match env {
        None => inbox_host_admit_allowed_open(),
        Some(env) => match admit_inbox_host(
            env,
            binding,
            host_key,
            activity_id,
            actor_uri,
            activity_type,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                console_error!(
                    "failed inbox host admission for host {host_key} activity_id={activity_id}: {error}"
                );
                inbox_host_admit_allowed_open()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_host_id_name_prefixes_peer_authority() {
        assert_eq!(
            inbox_host_id_name("mastodon.social"),
            "inbox-host:mastodon.social"
        );
        assert_eq!(
            inbox_host_id_name("social.example:8443"),
            "inbox-host:social.example:8443"
        );
    }

    #[test]
    fn fixed_window_admission_starts_new_window_when_empty() {
        let admission = fixed_window_admission(None, 0, 1_000_000, 60_000, 120);
        assert_eq!(
            admission,
            FixedWindowAdmission {
                window_start_ms: 1_000_000,
                count: 1,
                allowed: true,
            }
        );
    }

    #[test]
    fn fixed_window_admission_increments_within_window() {
        let admission = fixed_window_admission(Some(1_000_000), 5, 1_010_000, 60_000, 120);
        assert_eq!(
            admission,
            FixedWindowAdmission {
                window_start_ms: 1_000_000,
                count: 6,
                allowed: true,
            }
        );
    }

    #[test]
    fn fixed_window_admission_rejects_at_limit() {
        let admission = fixed_window_admission(Some(1_000_000), 120, 1_010_000, 60_000, 120);
        assert_eq!(
            admission,
            FixedWindowAdmission {
                window_start_ms: 1_000_000,
                count: 120,
                allowed: false,
            }
        );
    }

    #[test]
    fn fixed_window_admission_resets_after_window_elapsed() {
        let admission = fixed_window_admission(Some(1_000_000), 120, 1_070_000, 60_000, 120);
        assert_eq!(
            admission,
            FixedWindowAdmission {
                window_start_ms: 1_070_000,
                count: 1,
                allowed: true,
            }
        );
    }

    #[test]
    fn fixed_window_retry_after_secs_is_full_window_at_start() {
        assert_eq!(fixed_window_retry_after_secs(1_000_000, 1_000_000, 60_000), 60);
    }

    #[test]
    fn fixed_window_retry_after_secs_counts_down_within_window() {
        assert_eq!(fixed_window_retry_after_secs(1_000_000, 1_030_000, 60_000), 30);
        assert_eq!(fixed_window_retry_after_secs(1_000_000, 1_059_999, 60_000), 1);
    }

    #[test]
    fn fixed_window_retry_after_secs_minimum_is_one_when_denied() {
        assert_eq!(fixed_window_retry_after_secs(1_000_000, 1_060_000, 60_000), 1);
        assert_eq!(fixed_window_retry_after_secs(1_000_000, 2_000_000, 60_000), 1);
    }
}
