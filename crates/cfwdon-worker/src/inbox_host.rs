use serde::{Deserialize, Serialize};
use worker::{
    DurableObject, Env, Method, Request, RequestInit, Response, Result, State, console_error,
    durable_object,
};

const INBOX_HOST_ADMIT_PATH: &str = "/admit";
const INBOX_HOST_RELEASE_PATH: &str = "/release";
const INBOX_HOST_INTERNAL_ORIGIN: &str = "https://inbox-host";

/// Fixed-window admit window length (seconds).
pub(crate) const INBOX_HOST_ADMIT_WINDOW_SECS: u64 = 60;
/// Default max admits per window per remote host.
pub(crate) const INBOX_HOST_ADMIT_LIMIT: u64 = 120;
/// Max concurrent leased (in-flight) activities per remote host.
pub(crate) const INBOX_HOST_MAX_IN_FLIGHT: u64 = 32;
/// Retry-After hint when deny is due to in-flight backlog (not rate window).
pub(crate) const INBOX_HOST_BACKLOG_RETRY_AFTER_SECS: u64 = 5;
/// A lease older than this is treated as abandoned. Workers can die between
/// admit and release (CPU limit, eviction, panic), so leases must expire or the
/// backlog would deny that host forever.
pub(crate) const INBOX_HOST_LEASE_TTL_MS: u64 = 30_000;

const STORAGE_WINDOW_START_KEY: &str = "admit_window_start_ms";
const STORAGE_COUNT_KEY: &str = "admit_count";
const STORAGE_LEASES_KEY: &str = "in_flight_leases";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct InboxHostAdmitRequest {
    activity_id: Option<String>,
    actor_uri: Option<String>,
    activity_type: Option<String>,
    /// When true (default), a successful admit also takes an in-flight lease.
    /// Rate-only checks (e.g. shared-inbox AcceptedNoTargets) pass `lease: false`.
    #[serde(default = "default_lease_true")]
    lease: bool,
}

fn default_lease_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
struct InboxHostAdmitResponse {
    allowed: bool,
    /// True only when this admit took an in-flight lease that the caller must release.
    #[serde(default)]
    leased: bool,
    count: u64,
    in_flight: u64,
    retry_after_secs: u64,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct InboxHostReleaseResponse {
    in_flight: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InboxHostAdmitResult {
    pub allowed: bool,
    /// Release is only correct when this admit actually leased a slot.
    pub leased: bool,
    pub count: u32,
    pub in_flight: u32,
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
        if req.method() == Method::Post && path.ends_with(INBOX_HOST_RELEASE_PATH) {
            return self.handle_release().await;
        }

        Response::error("Not found", 404)
    }
}

impl InboxHost {
    async fn handle_admit(&self, mut req: Request) -> Result<Response> {
        let admit = req.json::<InboxHostAdmitRequest>().await?;
        let now_ms = inbox_host_now_millis();
        let window_ms = INBOX_HOST_ADMIT_WINDOW_SECS * 1000;

        let storage = self.state.storage();
        let window_start_ms = storage
            .get::<u64>(STORAGE_WINDOW_START_KEY)
            .await?
            .unwrap_or(now_ms);
        let count = storage.get::<u64>(STORAGE_COUNT_KEY).await?.unwrap_or(0);
        let stored_leases = storage
            .get::<Vec<u64>>(STORAGE_LEASES_KEY)
            .await?
            .unwrap_or_default();
        let mut leases = retain_live_leases(stored_leases.clone(), now_ms, INBOX_HOST_LEASE_TTL_MS);
        let expired_leases = leases.len() != stored_leases.len();
        let in_flight = leases.len() as u64;

        let admission = fixed_window_admission(
            Some(window_start_ms),
            count,
            now_ms,
            window_ms,
            INBOX_HOST_ADMIT_LIMIT,
        );

        if !admission.allowed {
            if expired_leases {
                storage.put(STORAGE_LEASES_KEY, &leases).await?;
            }
            return Response::from_json(&InboxHostAdmitResponse {
                allowed: false,
                leased: false,
                count: admission.count,
                in_flight,
                retry_after_secs: fixed_window_retry_after_secs(
                    admission.window_start_ms,
                    now_ms,
                    window_ms,
                ),
                reason: Some("rate_limited".to_owned()),
            });
        }

        if admit.lease && !backlog_admission(in_flight, INBOX_HOST_MAX_IN_FLIGHT) {
            if expired_leases {
                storage.put(STORAGE_LEASES_KEY, &leases).await?;
            }
            return Response::from_json(&InboxHostAdmitResponse {
                allowed: false,
                leased: false,
                count,
                in_flight,
                retry_after_secs: INBOX_HOST_BACKLOG_RETRY_AFTER_SECS,
                reason: Some("backlog".to_owned()),
            });
        }

        storage
            .put(STORAGE_WINDOW_START_KEY, admission.window_start_ms)
            .await?;
        storage.put(STORAGE_COUNT_KEY, admission.count).await?;

        if admit.lease {
            leases.push(now_ms);
        }
        if admit.lease || expired_leases {
            storage.put(STORAGE_LEASES_KEY, &leases).await?;
        }

        Response::from_json(&InboxHostAdmitResponse {
            allowed: true,
            leased: admit.lease,
            count: admission.count,
            in_flight: leases.len() as u64,
            retry_after_secs: 0,
            reason: None,
        })
    }

    async fn handle_release(&self) -> Result<Response> {
        let storage = self.state.storage();
        let now_ms = inbox_host_now_millis();
        let leases = storage
            .get::<Vec<u64>>(STORAGE_LEASES_KEY)
            .await?
            .unwrap_or_default();
        let leases =
            release_oldest_lease(retain_live_leases(leases, now_ms, INBOX_HOST_LEASE_TTL_MS));
        storage.put(STORAGE_LEASES_KEY, &leases).await?;
        Response::from_json(&InboxHostReleaseResponse {
            in_flight: leases.len() as u64,
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
    remaining_ms.div_ceil(1000).max(1)
}

fn inbox_host_admit_result_from_response(admit: InboxHostAdmitResponse) -> InboxHostAdmitResult {
    InboxHostAdmitResult {
        allowed: admit.allowed,
        leased: admit.leased,
        count: u32::try_from(admit.count).unwrap_or(u32::MAX),
        in_flight: u32::try_from(admit.in_flight).unwrap_or(u32::MAX),
        retry_after_secs: admit.retry_after_secs,
    }
}

/// Fail-open result used when the Durable Object is unreachable. It holds no
/// lease, so the caller must not release one.
pub(crate) fn inbox_host_admit_allowed_open() -> InboxHostAdmitResult {
    InboxHostAdmitResult {
        allowed: true,
        leased: false,
        count: 0,
        in_flight: 0,
        retry_after_secs: 0,
    }
}

/// Drops leases whose holder never released them within the TTL.
pub(crate) fn retain_live_leases(mut leases: Vec<u64>, now_ms: u64, lease_ttl_ms: u64) -> Vec<u64> {
    leases.retain(|acquired_ms| now_ms.saturating_sub(*acquired_ms) < lease_ttl_ms);
    leases
}

/// Releases the lease that has been held longest.
pub(crate) fn release_oldest_lease(mut leases: Vec<u64>) -> Vec<u64> {
    if leases.is_empty() {
        return leases;
    }
    let oldest = leases
        .iter()
        .enumerate()
        .min_by_key(|(_, acquired_ms)| **acquired_ms)
        .map(|(index, _)| index);
    if let Some(index) = oldest {
        leases.remove(index);
    }
    leases
}

pub(crate) async fn admit_inbox_host(
    env: &Env,
    binding: &str,
    host_key: &str,
    activity_id: &str,
    actor_uri: &str,
    activity_type: &str,
    lease: bool,
) -> Result<InboxHostAdmitResult> {
    let namespace = env.durable_object(binding)?;
    let object_name = inbox_host_id_name(host_key);
    let stub = namespace.get_by_name(&object_name)?;

    let body = serde_json::json!({
        "activity_id": activity_id,
        "actor_uri": actor_uri,
        "activity_type": activity_type,
        "lease": lease,
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
    lease: bool,
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
            lease,
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

pub(crate) async fn release_inbox_host(env: &Env, binding: &str, host_key: &str) -> Result<u64> {
    let namespace = env.durable_object(binding)?;
    let object_name = inbox_host_id_name(host_key);
    let stub = namespace.get_by_name(&object_name)?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post);

    let url = format!("{INBOX_HOST_INTERNAL_ORIGIN}{INBOX_HOST_RELEASE_PATH}");
    let request = Request::new_with_init(&url, &init)?;
    let mut response = stub.fetch_with_request(request).await?;
    if response.status_code() >= 400 {
        return Err(worker::Error::RustError(format!(
            "inbox host release failed with HTTP {}",
            response.status_code()
        )));
    }

    let release = response.json::<InboxHostReleaseResponse>().await?;
    Ok(release.in_flight)
}

pub(crate) async fn release_inbox_host_soft(env: Option<&Env>, binding: &str, host_key: &str) {
    let Some(env) = env else {
        return;
    };
    if let Err(error) = release_inbox_host(env, binding, host_key).await {
        console_error!("failed inbox host release for host {host_key}: {error}");
    }
}

/// Pure helper for backlog admission decisions (unit-tested without Durable Object storage).
fn backlog_admission(in_flight: u64, max_in_flight: u64) -> bool {
    in_flight < max_in_flight
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
        assert_eq!(
            fixed_window_retry_after_secs(1_000_000, 1_000_000, 60_000),
            60
        );
    }

    #[test]
    fn fixed_window_retry_after_secs_counts_down_within_window() {
        assert_eq!(
            fixed_window_retry_after_secs(1_000_000, 1_030_000, 60_000),
            30
        );
        assert_eq!(
            fixed_window_retry_after_secs(1_000_000, 1_059_999, 60_000),
            1
        );
    }

    #[test]
    fn fixed_window_retry_after_secs_minimum_is_one_when_denied() {
        assert_eq!(
            fixed_window_retry_after_secs(1_000_000, 1_060_000, 60_000),
            1
        );
        assert_eq!(
            fixed_window_retry_after_secs(1_000_000, 2_000_000, 60_000),
            1
        );
    }

    #[test]
    fn retain_live_leases_drops_expired_holders() {
        let leases = vec![1_000_000, 1_020_000, 1_040_000];
        assert_eq!(
            retain_live_leases(leases.clone(), 1_045_000, INBOX_HOST_LEASE_TTL_MS),
            vec![1_020_000, 1_040_000]
        );
        assert_eq!(
            retain_live_leases(leases.clone(), 1_040_000, INBOX_HOST_LEASE_TTL_MS),
            vec![1_020_000, 1_040_000]
        );
        assert!(retain_live_leases(leases, 2_000_000, INBOX_HOST_LEASE_TTL_MS).is_empty());
    }

    #[test]
    fn abandoned_leases_stop_denying_the_host_forever() {
        let saturated = (0..INBOX_HOST_MAX_IN_FLIGHT)
            .map(|_| 1_000_000)
            .collect::<Vec<_>>();
        assert!(!backlog_admission(
            retain_live_leases(saturated.clone(), 1_000_000, INBOX_HOST_LEASE_TTL_MS).len() as u64,
            INBOX_HOST_MAX_IN_FLIGHT
        ));
        // One TTL later the same stuck leases no longer block admission.
        assert!(backlog_admission(
            retain_live_leases(
                saturated,
                1_000_000 + INBOX_HOST_LEASE_TTL_MS,
                INBOX_HOST_LEASE_TTL_MS
            )
            .len() as u64,
            INBOX_HOST_MAX_IN_FLIGHT
        ));
    }

    #[test]
    fn release_oldest_lease_removes_one_holder() {
        assert_eq!(
            release_oldest_lease(vec![1_020_000, 1_000_000, 1_040_000]),
            vec![1_020_000, 1_040_000]
        );
        assert!(release_oldest_lease(vec![1_000_000]).is_empty());
        assert!(release_oldest_lease(Vec::new()).is_empty());
    }

    #[test]
    fn fail_open_admission_holds_no_lease() {
        let open = inbox_host_admit_allowed_open();
        assert!(open.allowed);
        assert!(!open.leased);
    }

    #[test]
    fn backlog_admission_rejects_at_max_in_flight() {
        assert!(backlog_admission(0, INBOX_HOST_MAX_IN_FLIGHT));
        assert!(backlog_admission(
            INBOX_HOST_MAX_IN_FLIGHT - 1,
            INBOX_HOST_MAX_IN_FLIGHT
        ));
        assert!(!backlog_admission(
            INBOX_HOST_MAX_IN_FLIGHT,
            INBOX_HOST_MAX_IN_FLIGHT
        ));
    }
}
