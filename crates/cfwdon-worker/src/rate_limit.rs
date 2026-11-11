//! Enforcing rate limits (Mastodon's Rack::Attack throttles and `RateLimiter`
//! families; the policy is `cfwdon_domain::rate_limit`). Each subject a
//! request counts against (its client address, its token or account, a
//! sending server) is one `RateLimitCounter` Durable Object call. See "Rate
//! Limits" in docs/reference/configuration.md.
//!
//! The limits fail open: a counter that cannot be reached lets the request
//! through, as an outage must not lock every client out.

mod counter;

use crate::auth::{LocalApiAuthentication, authenticate_local_api_request, request_cookie_value};
use crate::oauth_store::{
    AUTH0_SESSION_COOKIE, app_bearer_token_from_request, oauth_bearer_token_hash,
};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::rate_limit::{
    RateLimitCaller, RateLimitCheck, RateLimitCounterState, RateLimitHit, RateLimitPolicy,
    RateLimitPolicyError, RateLimitRequest, RateLimitVerdict, inbox_domain_check, ip_throttle_key,
    is_api_path, rate_limit_checks, rate_limit_verdict,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use worker::{
    Env, Method, ObjectNamespace, Request, RequestInit, Response, Result, console_error,
    console_warn,
};

/// The Durable Object binding in `wrangler.toml`. A Worker deployed without
/// it counts nothing.
const RATE_LIMIT_COUNTER_BINDING: &str = "RATE_LIMIT_COUNTER";
const COUNTER_HIT_URL: &str = "https://rate-limit-counter/hit";
const ENABLED_VAR: &str = "RATE_LIMITS_ENABLED";
const OVERRIDES_VAR: &str = "RATE_LIMITS";
/// Mastodon's `errors.429`.
const TOO_MANY_REQUESTS: &str = "Too many requests";
/// Entries each isolate-local memory keeps; beyond this the oldest go.
const MAX_REMEMBERED: usize = 10_000;
/// How long an isolate trusts who a credential belonged to. The handler
/// authenticates on its own; this only decides whose counter a request hits.
const CALLER_TTL_MS: u64 = 60_000;

/// The body the Worker posts to a `RateLimitCounter`.
#[derive(Debug, Serialize, Deserialize)]
struct CounterHitRequest {
    hits: Vec<RateLimitHit>,
    now_ms: u64,
}

/// What the middleware decided before the route runs.
pub(crate) enum RateLimitGate {
    /// Not counted, or counted outside the API: nothing to add to the response.
    Open,
    /// An API request under its limits: the counter with the least left, for
    /// the `X-RateLimit-*` headers.
    Counted(RateLimitCounterState),
    /// Over a limit: the 429 to answer with instead of routing.
    Refused(Response),
}

/// A small map that forgets its oldest entry once it holds `MAX_REMEMBERED`.
struct BoundedMemory<V> {
    entries: HashMap<String, V>,
    order: VecDeque<String>,
}

impl<V> BoundedMemory<V> {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&self, key: &str) -> Option<&V> {
        self.entries.get(key)
    }

    fn insert(&mut self, key: String, value: V) {
        if self.entries.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.entries.len() > MAX_REMEMBERED {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }
}

/// `(RATE_LIMITS_ENABLED, RATE_LIMITS)` as last read, and the policy they give.
type CachedPolicy = ((String, String), Option<RateLimitPolicy>);

thread_local! {
    static POLICY: RefCell<Option<CachedPolicy>> = const { RefCell::new(None) };
    /// `subject key \n counter name` -> when its window ends, for counters seen
    /// over their limit. Until then this isolate refuses without asking the
    /// Durable Object, so a flood from one client costs one call per window.
    static OVER: RefCell<BoundedMemory<u64>> = RefCell::new(BoundedMemory::new());
    /// Credential hash -> the caller it authenticated as, and until when.
    static CALLERS: RefCell<BoundedMemory<(RateLimitCaller, u64)>> =
        RefCell::new(BoundedMemory::new());
    static MISSING_BINDING_LOGGED: RefCell<bool> = const { RefCell::new(false) };
}

fn env_var(env: &Env, name: &str) -> String {
    env.var(name)
        .map(|value| value.to_string().trim().to_owned())
        .unwrap_or_default()
}

/// The limits from `RATE_LIMITS_ENABLED` (off for `false`, `0`, `off`, `no`)
/// and `RATE_LIMITS` (bucket overrides), or why the overrides do not parse.
fn policy_from_vars(
    enabled: &str,
    overrides: &str,
) -> std::result::Result<Option<RateLimitPolicy>, RateLimitPolicyError> {
    if matches!(
        enabled.to_ascii_lowercase().as_str(),
        "false" | "0" | "off" | "no"
    ) {
        return Ok(None);
    }
    if overrides.is_empty() {
        return Ok(Some(RateLimitPolicy::mastodon_defaults()));
    }
    RateLimitPolicy::with_overrides_json(overrides).map(Some)
}

fn rate_limit_policy(env: &Env) -> Option<RateLimitPolicy> {
    let vars = (env_var(env, ENABLED_VAR), env_var(env, OVERRIDES_VAR));
    POLICY.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((cached_vars, policy)) = slot.as_ref()
            && *cached_vars == vars
        {
            return policy.clone();
        }
        // Overrides that do not parse leave Mastodon's defaults in force.
        let policy = policy_from_vars(&vars.0, &vars.1).unwrap_or_else(|error| {
            console_warn!("{error}; using Mastodon's default rate limits");
            Some(RateLimitPolicy::mastodon_defaults())
        });
        *slot = Some((vars, policy.clone()));
        policy
    })
}

fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// The client address: `CF-Connecting-IP` only, which Cloudflare sets on
/// every request it forwards. `X-Forwarded-For` carries whatever the client sent.
fn client_ip_key(req: &Request) -> Option<String> {
    req.headers()
        .get("CF-Connecting-IP")
        .ok()
        .flatten()
        .and_then(|raw| ip_throttle_key(&raw))
}

/// The credential an API request presents: a bearer token, the Auth0 JWT
/// header, or the Auth0 session cookie.
fn request_credential(req: &Request, config: &AppConfig) -> Result<Option<String>> {
    if let Some(token) = app_bearer_token_from_request(req)? {
        return Ok(Some(token));
    }
    if let Some(value) = req.headers().get(&config.auth0_jwt_header)?
        && !value.trim().is_empty()
    {
        return Ok(Some(value.trim().to_owned()));
    }
    request_cookie_value(req, AUTH0_SESSION_COOKIE)
}

/// Who an API request counts as: a valid token's account or app; no
/// credential, or one that does not authenticate, counts as its address, as
/// in Mastodon.
async fn rate_limit_caller(req: &Request, env: &Env, config: &AppConfig) -> RateLimitCaller {
    let credential = match request_credential(req, config) {
        Ok(Some(credential)) => credential,
        Ok(None) | Err(_) => return RateLimitCaller::Anonymous,
    };
    let credential_hash = oauth_bearer_token_hash(&credential);
    let now = now_ms();
    let cached = CALLERS.with(|callers| {
        callers
            .borrow()
            .get(&credential_hash)
            .filter(|(_, expires_at_ms)| *expires_at_ms > now)
            .map(|(caller, _)| caller.clone())
    });
    if let Some(caller) = cached {
        return caller;
    }
    let authentication = match env.d1(&config.database_binding) {
        Ok(binding) => authenticate_local_api_request(req, &D1Database::new(binding), config).await,
        Err(error) => Err(error),
    };
    let caller = match authentication {
        Ok(LocalApiAuthentication::OAuthToken(auth)) => RateLimitCaller::Account {
            account_id: auth.account.id().to_owned(),
            token_id: auth.token.access_token,
        },
        Ok(LocalApiAuthentication::Auth0(account)) => RateLimitCaller::Account {
            account_id: account.id().to_owned(),
            token_id: credential_hash.clone(),
        },
        Ok(LocalApiAuthentication::PendingApproval(pending)) => RateLimitCaller::Account {
            account_id: pending.account.id().to_owned(),
            token_id: credential_hash.clone(),
        },
        Ok(LocalApiAuthentication::AppToken) => RateLimitCaller::Application {
            token_id: credential_hash.clone(),
        },
        Ok(LocalApiAuthentication::InvalidBearer | LocalApiAuthentication::None) => {
            RateLimitCaller::Anonymous
        }
        Err(error) => {
            console_warn!("rate limit caller lookup failed: {error}");
            return RateLimitCaller::Anonymous;
        }
    };
    CALLERS.with(|callers| {
        callers
            .borrow_mut()
            .insert(credential_hash, (caller.clone(), now + CALLER_TTL_MS));
    });
    caller
}

fn counter_namespace(env: &Env) -> Option<ObjectNamespace> {
    match env.durable_object(RATE_LIMIT_COUNTER_BINDING) {
        Ok(namespace) => Some(namespace),
        Err(_) => {
            MISSING_BINDING_LOGGED.with(|logged| {
                if !logged.replace(true) {
                    console_warn!(
                        "rate limits are on but the {RATE_LIMIT_COUNTER_BINDING} binding is missing; nothing is counted"
                    );
                }
            });
            None
        }
    }
}

async fn hit_counter(
    namespace: &ObjectNamespace,
    subject_key: &str,
    hits: &[RateLimitHit],
    now_ms: u64,
) -> Result<Vec<RateLimitCounterState>> {
    let body = serde_json::to_string(&CounterHitRequest {
        hits: hits.to_vec(),
        now_ms,
    })
    .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let headers = worker::Headers::new();
    headers.set("Content-Type", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body)));
    let request = Request::new_with_init(COUNTER_HIT_URL, &init)?;
    let mut response = namespace
        .get_by_name(subject_key)?
        .fetch_with_request(request)
        .await?;
    if response.status_code() != 200 {
        return Err(worker::Error::RustError(format!(
            "rate limit counter answered HTTP {}",
            response.status_code()
        )));
    }
    response.json().await
}

fn over_key(subject_key: &str, counter: &str) -> String {
    format!("{subject_key}\n{counter}")
}

/// A counter this isolate already saw over its limit, as the state to refuse with.
fn remembered_over(
    subject_key: &str,
    hits: &[RateLimitHit],
    now_ms: u64,
) -> Option<RateLimitCounterState> {
    OVER.with(|over| {
        let over = over.borrow();
        hits.iter().find_map(|hit| {
            let reset_at_ms = *over.get(&over_key(subject_key, &hit.name))?;
            (reset_at_ms > now_ms).then(|| RateLimitCounterState {
                name: hit.name.clone(),
                limit: hit.limit,
                count: hit.limit.saturating_add(1),
                reset_at_ms,
            })
        })
    })
}

async fn run_check(
    namespace: &ObjectNamespace,
    check: &RateLimitCheck,
    now_ms: u64,
) -> Vec<RateLimitCounterState> {
    let subject_key = check.subject.key();
    if let Some(state) = remembered_over(&subject_key, &check.hits, now_ms) {
        return vec![state];
    }
    match hit_counter(namespace, &subject_key, &check.hits, now_ms).await {
        Ok(states) => {
            OVER.with(|over| {
                let mut over = over.borrow_mut();
                for state in states.iter().filter(|state| state.is_over()) {
                    over.insert(over_key(&subject_key, &state.name), state.reset_at_ms);
                }
            });
            states
        }
        Err(error) => {
            console_error!("rate limit counter unavailable; letting the request through: {error}");
            Vec::new()
        }
    }
}

async fn run_checks(env: &Env, checks: &[RateLimitCheck]) -> RateLimitVerdict {
    let now = now_ms();
    let Some(namespace) = counter_namespace(env) else {
        return RateLimitVerdict::Allowed { headline: None };
    };
    let states = futures_util::future::join_all(
        checks.iter().map(|check| run_check(&namespace, check, now)),
    )
    .await;
    rate_limit_verdict(&states.concat(), now)
}

fn reset_header_value(reset_at_ms: u64) -> String {
    i64::try_from(reset_at_ms / 1000)
        .ok()
        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok())
        .and_then(|reset| reset.format(&Rfc3339).ok())
        .unwrap_or_default()
}

fn rate_limit_headers(state: &RateLimitCounterState) -> [(&'static str, String); 3] {
    [
        ("X-RateLimit-Limit", state.limit.to_string()),
        ("X-RateLimit-Remaining", state.remaining().to_string()),
        ("X-RateLimit-Reset", reset_header_value(state.reset_at_ms)),
    ]
}

pub(crate) fn apply_rate_limit_headers(
    response: &mut Response,
    state: &RateLimitCounterState,
) -> Result<()> {
    for (name, value) in rate_limit_headers(state) {
        response.headers_mut().set(name, &value)?;
    }
    Ok(())
}

/// Mastodon's throttled response: 429, `{"error":"Too many requests"}`, the
/// limit headers and `Retry-After`.
fn too_many_requests(
    headline: &RateLimitCounterState,
    retry_after_seconds: u64,
) -> Result<Response> {
    let mut response =
        Response::from_json(&serde_json::json!({ "error": TOO_MANY_REQUESTS }))?.with_status(429);
    apply_rate_limit_headers(&mut response, headline)?;
    response
        .headers_mut()
        .set("Retry-After", &retry_after_seconds.to_string())?;
    Ok(response)
}

/// Count the request against its buckets before routing. Refuses with 429
/// once one is over; otherwise says which counter the `X-RateLimit-*`
/// headers of an API response describe.
pub(crate) async fn enforce_rate_limits(
    req: &Request,
    env: &Env,
    config: &AppConfig,
    method: &str,
    path: &str,
) -> Result<RateLimitGate> {
    let Some(policy) = rate_limit_policy(env) else {
        return Ok(RateLimitGate::Open);
    };
    let ip = client_ip_key(req);
    let url = req.url().ok();
    let caller = if is_api_path(path) {
        rate_limit_caller(req, env, config).await
    } else {
        RateLimitCaller::Anonymous
    };
    let checks = rate_limit_checks(
        &policy,
        &RateLimitRequest {
            method,
            path,
            query: url.as_ref().and_then(|url| url.query()).unwrap_or(""),
            local_domain: &config.instance_domain,
            ip: ip.as_deref(),
            caller: &caller,
        },
    );
    if checks.is_empty() {
        return Ok(RateLimitGate::Open);
    }
    Ok(match run_checks(env, &checks).await {
        RateLimitVerdict::Limited {
            headline,
            retry_after_seconds,
        } => RateLimitGate::Refused(too_many_requests(&headline, retry_after_seconds)?),
        RateLimitVerdict::Allowed {
            headline: Some(headline),
        } if is_api_path(path) => RateLimitGate::Counted(headline),
        RateLimitVerdict::Allowed { .. } => RateLimitGate::Open,
    })
}

/// The `inbox_domain` limit for a delivery whose signature verified, keyed on
/// the server that signed it: a 429 when that server is over. Remote servers
/// retry a 429 later, so nothing is lost.
pub(crate) async fn limit_inbox_delivery(
    env: &Env,
    signer_actor_uri: &str,
    signer_domain: &str,
) -> Result<Option<Response>> {
    let Some(policy) = rate_limit_policy(env) else {
        return Ok(None);
    };
    let host = url::Url::parse(signer_actor_uri)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| signer_domain.to_owned());
    let Some(check) = inbox_domain_check(&policy, &host) else {
        return Ok(None);
    };
    match run_checks(env, &[check]).await {
        RateLimitVerdict::Limited {
            headline,
            retry_after_seconds,
        } => Ok(Some(too_many_requests(&headline, retry_after_seconds)?)),
        RateLimitVerdict::Allowed { .. } => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundedMemory, MAX_REMEMBERED, policy_from_vars, rate_limit_headers};
    use cfwdon_domain::rate_limit::{
        RateLimitBucket, RateLimitCounterState, RateLimitPolicy, RateLimitPolicyError,
        RateLimitRule,
    };

    #[test]
    fn limits_are_on_by_default_and_off_for_false_like_values() {
        assert_eq!(
            policy_from_vars("", ""),
            Ok(Some(RateLimitPolicy::mastodon_defaults()))
        );
        assert_eq!(
            policy_from_vars("true", ""),
            Ok(Some(RateLimitPolicy::mastodon_defaults()))
        );
        for off in ["false", "0", "OFF", "no"] {
            assert_eq!(
                policy_from_vars(off, r#"{"api_ipp": false}"#),
                Ok(None),
                "{off}"
            );
        }
    }

    #[test]
    fn overrides_apply_and_a_malformed_value_is_refused() {
        let policy = policy_from_vars("", r#"{"api_ip": false}"#)
            .expect("valid overrides")
            .expect("enabled");
        assert_eq!(
            policy.rule(RateLimitBucket::ApiIp),
            RateLimitRule::Unlimited
        );
        assert_eq!(
            policy_from_vars("", r#"{"api_ipp": false}"#),
            Err(RateLimitPolicyError::UnknownBucket("api_ipp".to_owned()))
        );
    }

    #[test]
    fn headers_carry_limit_remaining_and_an_iso_reset_time() {
        let headers = rate_limit_headers(&RateLimitCounterState {
            name: "api_ip".to_owned(),
            limit: 300,
            count: 301,
            reset_at_ms: 1_793_232_300_000,
        });
        assert_eq!(
            headers,
            [
                ("X-RateLimit-Limit", "300".to_owned()),
                ("X-RateLimit-Remaining", "0".to_owned()),
                ("X-RateLimit-Reset", "2026-10-29T00:05:00Z".to_owned()),
            ]
        );
    }

    #[test]
    fn bounded_memory_forgets_the_oldest_entries() {
        let mut memory = BoundedMemory::new();
        for index in 0..=MAX_REMEMBERED {
            memory.insert(index.to_string(), index);
        }
        assert_eq!(memory.get("0"), None);
        assert_eq!(memory.get("1"), Some(&1));
        assert_eq!(
            memory.get(&MAX_REMEMBERED.to_string()),
            Some(&MAX_REMEMBERED)
        );
    }
}
