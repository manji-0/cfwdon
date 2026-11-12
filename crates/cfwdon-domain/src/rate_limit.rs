//! Rate limits, after Mastodon's Rack::Attack throttles and its `RateLimiter`
//! families. This module is the policy: which counters a request counts
//! against, how a fixed window advances, and what the counts mean. Where the
//! counts live is the worker's business (the `RateLimitCounter` Durable
//! Object).
//!
//! Every counter is a fixed window aligned to the epoch, as Rack::Attack's
//! are: a 5-minute window starts on a multiple of 300 seconds, and its reset
//! is the next multiple.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;

/// The throttles, by the names `RATE_LIMITS` overrides them under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RateLimitBucket {
    /// Unauthenticated `/api/*` requests, per client address (`throttle_unauthenticated_api`).
    ApiIp,
    /// `/api/*` requests per OAuth token (`throttle_per_token_api`).
    ApiToken,
    /// `/api/*` requests per signed-in account, over all its tokens (`throttle_authenticated_api`).
    ApiAccount,
    /// Media uploads per account (`throttle_api_media`, `RateLimiter` `media`).
    Media,
    /// Statuses posted per account (`RateLimiter` `statuses`).
    Statuses,
    /// Boosts per account (`RateLimiter` `reblogs`).
    Reblogs,
    /// Follows per account (`RateLimiter` `follows`).
    Follows,
    /// Reports filed per account (`RateLimiter` `reports`).
    Reports,
    /// OAuth authorization and the Auth0 sign-in callback per client address
    /// (`throttle_login_attempts/ip`; the callback is where sign-ups happen).
    SignIn,
    /// OAuth app registrations per client address.
    AppRegistration,
    /// OAuth token requests per client address.
    OauthToken,
    /// WebFinger, NodeInfo and actor/object fetches per client address.
    FederationFetch,
    /// Inbox deliveries per client address, before the signature is checked.
    InboxIp,
    /// Inbox deliveries per sending server, once its signature verified.
    InboxDomain,
    /// Lookups that make this server fetch a remote account: `resolve=true`
    /// searches and `accounts/lookup` of a remote `acct`. Per account, or per
    /// client address when anonymous.
    RemoteLookup,
}

impl RateLimitBucket {
    pub const ALL: [Self; 15] = [
        Self::ApiIp,
        Self::ApiToken,
        Self::ApiAccount,
        Self::Media,
        Self::Statuses,
        Self::Reblogs,
        Self::Follows,
        Self::Reports,
        Self::SignIn,
        Self::AppRegistration,
        Self::OauthToken,
        Self::FederationFetch,
        Self::InboxIp,
        Self::InboxDomain,
        Self::RemoteLookup,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ApiIp => "api_ip",
            Self::ApiToken => "api_token",
            Self::ApiAccount => "api_account",
            Self::Media => "media",
            Self::Statuses => "statuses",
            Self::Reblogs => "reblogs",
            Self::Follows => "follows",
            Self::Reports => "reports",
            Self::SignIn => "sign_in",
            Self::AppRegistration => "app_registration",
            Self::OauthToken => "oauth_token",
            Self::FederationFetch => "federation_fetch",
            Self::InboxIp => "inbox_ip",
            Self::InboxDomain => "inbox_domain",
            Self::RemoteLookup => "remote_lookup",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|bucket| bucket.name() == name)
    }

    fn index(self) -> usize {
        self as usize
    }

    /// Where the bucket's counters are counted. The traffic-volume buckets
    /// (API requests, federation fetches, inbox deliveries) count in the
    /// isolate first ([`plan_hits`]); every other bucket guards an action or a
    /// credential, or spans a window longer than an isolate is likely to live,
    /// and asks the shared counter on every hit. tstodon's
    /// `server/rate-limit.ts` keeps the same split.
    pub fn counting(self) -> RateLimitCounting {
        match self {
            Self::ApiIp
            | Self::ApiToken
            | Self::ApiAccount
            | Self::FederationFetch
            | Self::InboxIp
            | Self::InboxDomain => RateLimitCounting::LocalFirst,
            Self::Media
            | Self::Statuses
            | Self::Reblogs
            | Self::Follows
            | Self::Reports
            | Self::SignIn
            | Self::AppRegistration
            | Self::OauthToken
            | Self::RemoteLookup => RateLimitCounting::Exact,
        }
    }
}

/// Whether a counter may be counted in the isolate before the shared counter
/// (the `RateLimitCounter` Durable Object) is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateLimitCounting {
    /// Counted in the isolate while it stays well under its limit; see [`plan_hits`].
    LocalFirst,
    /// Every hit goes to the shared counter.
    Exact,
}

/// One bucket's limit, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitRule {
    Throttle { limit: u32, period_seconds: u32 },
    Unlimited,
}

const MINUTES: u32 = 60;
const HOURS: u32 = 60 * MINUTES;

const fn throttle(limit: u32, period_seconds: u32) -> RateLimitRule {
    RateLimitRule::Throttle {
        limit,
        period_seconds,
    }
}

/// Why a `RATE_LIMITS` value was refused as a whole.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RateLimitPolicyError {
    #[error("RATE_LIMITS is not a JSON object")]
    NotAnObject,
    #[error("RATE_LIMITS names an unknown bucket `{0}`")]
    UnknownBucket(String),
    #[error("RATE_LIMITS has a malformed rule for `{0}`")]
    MalformedRule(String),
}

/// The rule of every bucket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimitPolicy {
    rules: [RateLimitRule; RateLimitBucket::ALL.len()],
}

impl RateLimitPolicy {
    /// Mastodon's numbers where it has one; see docs/reference/configuration.md.
    pub fn mastodon_defaults() -> Self {
        let rule = |bucket| match bucket {
            RateLimitBucket::ApiIp | RateLimitBucket::ApiToken => throttle(300, 5 * MINUTES),
            RateLimitBucket::ApiAccount => throttle(1500, 5 * MINUTES),
            RateLimitBucket::Media => throttle(30, 30 * MINUTES),
            RateLimitBucket::Statuses | RateLimitBucket::Reblogs => throttle(300, 3 * HOURS),
            RateLimitBucket::Follows | RateLimitBucket::Reports => throttle(400, 24 * HOURS),
            RateLimitBucket::SignIn | RateLimitBucket::OauthToken => throttle(25, 5 * MINUTES),
            RateLimitBucket::AppRegistration => throttle(5, 10 * MINUTES),
            RateLimitBucket::FederationFetch | RateLimitBucket::InboxIp => {
                throttle(1500, 5 * MINUTES)
            }
            RateLimitBucket::InboxDomain => throttle(3000, 5 * MINUTES),
            RateLimitBucket::RemoteLookup => throttle(60, 5 * MINUTES),
        };
        Self {
            rules: RateLimitBucket::ALL.map(rule),
        }
    }

    /// The defaults with the operator's `RATE_LIMITS` JSON applied: an object
    /// of bucket names, each `{"limit": n, "period": seconds}` or `false` to
    /// lift the bucket. Unknown bucket names fail the whole value, so a typo
    /// is not silently a default.
    pub fn with_overrides_json(raw: &str) -> Result<Self, RateLimitPolicyError> {
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| RateLimitPolicyError::NotAnObject)?;
        let serde_json::Value::Object(overrides) = value else {
            return Err(RateLimitPolicyError::NotAnObject);
        };
        let mut policy = Self::mastodon_defaults();
        for (name, rule) in overrides {
            let bucket = RateLimitBucket::from_name(&name)
                .ok_or_else(|| RateLimitPolicyError::UnknownBucket(name.clone()))?;
            let rule = parse_override(&rule).ok_or(RateLimitPolicyError::MalformedRule(name))?;
            policy.rules[bucket.index()] = rule;
        }
        Ok(policy)
    }

    pub fn rule(&self, bucket: RateLimitBucket) -> RateLimitRule {
        self.rules[bucket.index()]
    }

    fn hit(&self, bucket: RateLimitBucket, name: String) -> Option<RateLimitHit> {
        match self.rule(bucket) {
            RateLimitRule::Throttle {
                limit,
                period_seconds,
            } => Some(RateLimitHit {
                name,
                limit,
                period_ms: u64::from(period_seconds) * 1000,
                counting: bucket.counting(),
            }),
            RateLimitRule::Unlimited => None,
        }
    }

    fn bucket_hit(&self, bucket: RateLimitBucket) -> Option<RateLimitHit> {
        self.hit(bucket, bucket.name().to_owned())
    }
}

fn parse_override(rule: &serde_json::Value) -> Option<RateLimitRule> {
    if rule == &serde_json::Value::Bool(false) {
        return Some(RateLimitRule::Unlimited);
    }
    let fields = rule.as_object()?;
    if fields.len() != 2 {
        return None;
    }
    let positive = |key: &str| {
        fields
            .get(key)?
            .as_u64()
            .filter(|value| *value > 0)
            .and_then(|value| u32::try_from(value).ok())
    };
    Some(throttle(positive("limit")?, positive("period")?))
}

/// Whose counters a request counts against. Each subject's counters are kept
/// together, so one request costs one counter call per subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RateLimitSubject {
    /// A client address's throttle key (`ip_throttle_key`).
    Ip(String),
    Account(String),
    /// An OAuth app token that acts for no account.
    Token(String),
    /// A sending server's host.
    RemoteDomain(String),
}

impl RateLimitSubject {
    /// The subject's storage key: `ip:…`, `account:…`, `token:…` or `domain:…`.
    pub fn key(&self) -> String {
        match self {
            Self::Ip(key) => format!("ip:{key}"),
            Self::Account(account_id) => format!("account:{account_id}"),
            Self::Token(token_id) => format!("token:{token_id}"),
            Self::RemoteDomain(host) => format!("domain:{}", host.to_ascii_lowercase()),
        }
    }
}

/// One counter a request increments, with the limit it is held to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitHit {
    /// The counter's name within its subject (the bucket, or `api_token:<id>`).
    pub name: String,
    pub limit: u32,
    pub period_ms: u64,
    pub counting: RateLimitCounting,
}

/// What an isolate sends the shared counter for one counter: the hit, and how
/// many hits it stands for (this one and those the isolate counted locally
/// since it last asked).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitIncrement {
    pub hit: RateLimitHit,
    pub by: u32,
}

/// The counters a request increments under one subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimitCheck {
    pub subject: RateLimitSubject,
    pub hits: Vec<RateLimitHit>,
}

/// A counter after a hit: its count in the current window, and when the window ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitCounterState {
    pub name: String,
    pub limit: u32,
    pub count: u32,
    pub reset_at_ms: u64,
}

impl RateLimitCounterState {
    pub fn remaining(&self) -> u32 {
        self.limit.saturating_sub(self.count)
    }

    pub fn is_over(&self) -> bool {
        self.count > self.limit
    }
}

/// Who sent a request, as far as rate limits care.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RateLimitCaller {
    Anonymous,
    /// A valid app token with no account (`client_credentials`).
    Application {
        token_id: String,
    },
    Account {
        account_id: String,
        token_id: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct RateLimitRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    /// The raw query string, without the leading `?`.
    pub query: &'a str,
    /// This instance's own domain: an `acct` on it is not a remote lookup.
    pub local_domain: &'a str,
    /// The client address's throttle key (`ip_throttle_key`), when Cloudflare named one.
    pub ip: Option<&'a str>,
    pub caller: &'a RateLimitCaller,
}

/// Mastodon's `api_request?`: the REST API. The streaming endpoint is not
/// counted (Mastodon serves it from a separate server).
pub fn is_api_path(path: &str) -> bool {
    (path == "/api" || path.starts_with("/api/")) && !path.starts_with("/api/v1/streaming")
}

/// The path's segments without the empty one a trailing slash leaves.
fn segments(path: &str) -> Vec<&str> {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let trimmed = trimmed.strip_suffix('/').unwrap_or(trimmed);
    trimmed.split('/').collect()
}

/// Whether `path` matches `pattern`, where a `*` segment is any one non-empty segment.
fn matches(path: &str, pattern: &[&str]) -> bool {
    let actual = segments(path);
    actual.len() == pattern.len()
        && actual
            .iter()
            .zip(pattern)
            .all(|(segment, expected)| match *expected {
                "*" => !segment.is_empty(),
                literal => *segment == literal,
            })
}

/// The decoded value of query parameter `key` (`+` is a space, `%XX` a byte).
fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (percent_decode(name) == key).then(|| percent_decode(value))
    })
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' => {
                match raw
                    .get(index + 1..index + 3)
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                {
                    Some(byte) => {
                        out.push(byte);
                        index += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether the request makes the server fetch a remote account: a search with
/// `resolve=true`, or a lookup of an `acct` naming another domain.
fn is_remote_lookup(request: &RateLimitRequest<'_>) -> bool {
    if request.method != "GET" {
        return false;
    }
    let path = request.path;
    if matches(path, &["api", "v2", "search"])
        || matches(path, &["api", "v1", "accounts", "search"])
    {
        return matches!(
            query_value(request.query, "resolve").as_deref(),
            Some("true" | "1" | "t" | "on" | "yes")
        );
    }
    if matches(path, &["api", "v1", "accounts", "lookup"]) {
        let Some(acct) = query_value(request.query, "acct") else {
            return false;
        };
        return acct
            .trim_start_matches('@')
            .split_once('@')
            .is_some_and(|(_, domain)| !domain.eq_ignore_ascii_case(request.local_domain));
    }
    false
}

fn is_inbox_path(path: &str) -> bool {
    matches(path, &["inbox"])
        || matches(path, &["users", "*", "inbox"])
        || matches(path, &["actor", "inbox"])
}

fn is_federation_fetch_path(path: &str) -> bool {
    path.starts_with("/.well-known/")
        || path == "/nodeinfo"
        || path.starts_with("/nodeinfo/")
        || path.starts_with("/users/")
        || path == "/actor"
        || path.starts_with("/actor/")
}

/// The per-account action family a request counts against (Mastodon's `RateLimiter`).
fn account_family(method: &str, path: &str) -> Option<RateLimitBucket> {
    if method != "POST" {
        return None;
    }
    if matches(path, &["api", "v1", "media"]) || matches(path, &["api", "v2", "media"]) {
        Some(RateLimitBucket::Media)
    } else if matches(path, &["api", "v1", "statuses"]) {
        Some(RateLimitBucket::Statuses)
    } else if matches(path, &["api", "v1", "statuses", "*", "reblog"]) {
        Some(RateLimitBucket::Reblogs)
    } else if matches(path, &["api", "v1", "accounts", "*", "follow"]) {
        Some(RateLimitBucket::Follows)
    } else if matches(path, &["api", "v1", "reports"]) {
        Some(RateLimitBucket::Reports)
    } else {
        None
    }
}

/// The address-keyed buckets a request counts against.
fn ip_buckets(request: &RateLimitRequest<'_>) -> Vec<RateLimitBucket> {
    let RateLimitRequest {
        method,
        path,
        caller,
        ..
    } = *request;
    let mut buckets = Vec::new();
    if *caller == RateLimitCaller::Anonymous && is_api_path(path) {
        buckets.push(RateLimitBucket::ApiIp);
        if is_remote_lookup(request) {
            buckets.push(RateLimitBucket::RemoteLookup);
        }
    }
    match method {
        "POST" => {
            if matches(path, &["api", "v1", "apps"]) {
                buckets.push(RateLimitBucket::AppRegistration);
            } else if path == "/oauth/token" {
                buckets.push(RateLimitBucket::OauthToken);
            } else if path == "/oauth/authorize" {
                buckets.push(RateLimitBucket::SignIn);
            } else if is_inbox_path(path) {
                buckets.push(RateLimitBucket::InboxIp);
            }
        }
        "GET" | "HEAD" => {
            if path == "/oauth/authorize" || path == "/oauth/auth0/callback" {
                buckets.push(RateLimitBucket::SignIn);
            } else if is_federation_fetch_path(path) {
                buckets.push(RateLimitBucket::FederationFetch);
            }
        }
        _ => {}
    }
    buckets
}

fn check_of(subject: RateLimitSubject, hits: Vec<Option<RateLimitHit>>) -> Option<RateLimitCheck> {
    let hits: Vec<RateLimitHit> = hits.into_iter().flatten().collect();
    (!hits.is_empty()).then_some(RateLimitCheck { subject, hits })
}

fn caller_check(
    policy: &RateLimitPolicy,
    request: &RateLimitRequest<'_>,
) -> Option<RateLimitCheck> {
    if !is_api_path(request.path) {
        return None;
    }
    match request.caller {
        RateLimitCaller::Anonymous => None,
        RateLimitCaller::Application { token_id } => check_of(
            RateLimitSubject::Token(token_id.clone()),
            vec![
                policy.bucket_hit(RateLimitBucket::ApiToken),
                is_remote_lookup(request)
                    .then(|| policy.bucket_hit(RateLimitBucket::RemoteLookup))
                    .flatten(),
            ],
        ),
        RateLimitCaller::Account {
            account_id,
            token_id,
        } => {
            let mut hits = vec![
                policy.hit(RateLimitBucket::ApiToken, format!("api_token:{token_id}")),
                policy.bucket_hit(RateLimitBucket::ApiAccount),
            ];
            hits.extend(
                account_family(request.method, request.path)
                    .map(|bucket| policy.bucket_hit(bucket)),
            );
            if is_remote_lookup(request) {
                hits.push(policy.bucket_hit(RateLimitBucket::RemoteLookup));
            }
            check_of(RateLimitSubject::Account(account_id.clone()), hits)
        }
    }
}

/// The checks a request counts against, one per subject: its client address
/// (when known) for the address-keyed buckets, and its token or account for
/// the API buckets and the action families. Mastodon counts an
/// authenticated API request against the user and the token, not the
/// address, and does not exempt staff; neither does this.
pub fn rate_limit_checks(
    policy: &RateLimitPolicy,
    request: &RateLimitRequest<'_>,
) -> Vec<RateLimitCheck> {
    let ip_check = request.ip.and_then(|ip| {
        check_of(
            RateLimitSubject::Ip(ip.to_owned()),
            ip_buckets(request)
                .into_iter()
                .map(|bucket| policy.bucket_hit(bucket))
                .collect(),
        )
    });
    ip_check
        .into_iter()
        .chain(caller_check(policy, request))
        .collect()
}

/// The check for an inbox delivery whose signature verified, keyed on the
/// sending server, so unsigned or forged deliveries cannot spend a server's
/// budget.
pub fn inbox_domain_check(policy: &RateLimitPolicy, host: &str) -> Option<RateLimitCheck> {
    check_of(
        RateLimitSubject::RemoteDomain(host.to_owned()),
        vec![policy.bucket_hit(RateLimitBucket::InboxDomain)],
    )
}

/// One stored counter: its window's start, the window's length and its count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCounter {
    pub start_ms: u64,
    pub period_ms: u64,
    pub count: u32,
}

impl StoredCounter {
    fn end_ms(&self) -> u64 {
        self.start_ms.saturating_add(self.period_ms)
    }
}

/// One subject's counters as stored, by counter name.
pub type RateLimitCounterTable = BTreeMap<String, StoredCounter>;

/// The start of the fixed, epoch-aligned window of `period_ms` that `now_ms` is in.
fn window_start_ms(now_ms: u64, period_ms: u64) -> u64 {
    now_ms - now_ms % period_ms.max(1)
}

/// Count `increments` at `now_ms`: each counter in its current window goes up
/// by the increment, a counter whose window has passed starts again from it,
/// and windows that have ended are dropped from the table. Returns each
/// counter's state after the hit, in the order of `increments`.
pub fn count_hits(
    table: &mut RateLimitCounterTable,
    increments: &[RateLimitIncrement],
    now_ms: u64,
) -> Vec<RateLimitCounterState> {
    table.retain(|_, stored| stored.end_ms() > now_ms);
    increments
        .iter()
        .map(|RateLimitIncrement { hit, by }| {
            let start_ms = window_start_ms(now_ms, hit.period_ms);
            let count = match table.get(&hit.name) {
                Some(stored)
                    if stored.start_ms == start_ms && stored.period_ms == hit.period_ms =>
                {
                    stored.count.saturating_add(*by)
                }
                _ => *by,
            };
            let stored = StoredCounter {
                start_ms,
                period_ms: hit.period_ms,
                count,
            };
            let reset_at_ms = stored.end_ms();
            table.insert(hit.name.clone(), stored);
            RateLimitCounterState {
                name: hit.name.clone(),
                limit: hit.limit,
                count,
                reset_at_ms,
            }
        })
        .collect()
}

/// When the last window in `table` ends, or `None` for an empty table.
pub fn table_expires_at_ms(table: &RateLimitCounterTable) -> Option<u64> {
    table.values().map(StoredCounter::end_ms).max()
}

/// One counter as one isolate has seen it in the current window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalCounter {
    start_ms: u64,
    period_ms: u64,
    /// Hits this isolate counted.
    seen: u32,
    /// Of those, the hits not yet sent to the shared counter.
    unsent: u32,
    /// The count the shared counter last answered, every isolate's hits included.
    shared: u32,
}

impl LocalCounter {
    /// The counter in the window `now_ms` is in, started afresh if its window passed.
    fn current(&self, hit: &RateLimitHit, now_ms: u64) -> Self {
        let start_ms = window_start_ms(now_ms, hit.period_ms);
        if self.start_ms == start_ms && self.period_ms == hit.period_ms {
            self.clone()
        } else {
            Self {
                start_ms,
                period_ms: hit.period_ms,
                ..Self::default()
            }
        }
    }

    /// The best count this isolate knows of.
    fn estimate(&self) -> u32 {
        self.seen.max(self.shared.saturating_add(self.unsent))
    }

    fn state(&self, hit: &RateLimitHit, count: u32) -> RateLimitCounterState {
        RateLimitCounterState {
            name: hit.name.clone(),
            limit: hit.limit,
            count,
            reset_at_ms: self.start_ms.saturating_add(self.period_ms),
        }
    }

    /// Whether one more hit may be counted here without asking: a local-first
    /// counter may grow by a quarter of the headroom the shared counter last
    /// left (a quarter of the limit before it was asked). Near the limit the
    /// quarter rounds to nothing, so the shared counter decides every hit there.
    fn may_count_locally(&self, hit: &RateLimitHit) -> bool {
        let headroom = hit.limit.saturating_sub(self.shared);
        hit.counting == RateLimitCounting::LocalFirst
            && self.estimate() < self.shared.saturating_add(headroom / 4)
    }

    /// What the shared counter answered for this counter.
    pub fn record_shared(&mut self, state: &RateLimitCounterState) {
        if self.start_ms.saturating_add(self.period_ms) == state.reset_at_ms {
            self.shared = self.shared.max(state.count);
        }
    }
}

/// What a check's hits need once the isolate's counters have seen them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalPlan {
    /// Decided in the isolate: the counters' states after the hit, or the
    /// counter the shared counter already found over its limit.
    Answered(Vec<RateLimitCounterState>),
    /// Ask the shared counter, sending these increments.
    Ask(Vec<RateLimitIncrement>),
}

/// Count one subject's `hits` in the isolate's `counters` (one per hit, in
/// order; a default counter for one not seen yet), or decide to ask the
/// shared counter.
///
/// A counter the shared counter found over its limit refuses until its window
/// ends without asking again. When every hit may still be counted locally
/// ([`LocalCounter::may_count_locally`]), the request costs no call;
/// otherwise the call carries every hit counted locally since the last one,
/// so the shared count misses only what isolates that never asked counted,
/// each under a quarter of its limit.
pub fn plan_hits(counters: &mut [LocalCounter], hits: &[RateLimitHit], now_ms: u64) -> LocalPlan {
    for (counter, hit) in counters.iter_mut().zip(hits) {
        *counter = counter.current(hit, now_ms);
    }
    if let Some((counter, hit)) = counters
        .iter()
        .zip(hits)
        .find(|(counter, hit)| counter.shared > hit.limit)
    {
        return LocalPlan::Answered(vec![counter.state(hit, counter.shared)]);
    }
    let local = counters
        .iter()
        .zip(hits)
        .all(|(counter, hit)| counter.may_count_locally(hit));
    let pairs = counters.iter_mut().zip(hits);
    if local {
        LocalPlan::Answered(
            pairs
                .map(|(counter, hit)| {
                    counter.seen += 1;
                    counter.unsent += 1;
                    counter.state(hit, counter.estimate())
                })
                .collect(),
        )
    } else {
        LocalPlan::Ask(
            pairs
                .map(|(counter, hit)| {
                    counter.seen = counter.seen.saturating_add(1);
                    let by = std::mem::take(&mut counter.unsent) + 1;
                    RateLimitIncrement {
                        hit: hit.clone(),
                        by,
                    }
                })
                .collect(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RateLimitVerdict {
    /// Under every limit; `headline` is the counter with the least left, for the headers.
    Allowed {
        headline: Option<RateLimitCounterState>,
    },
    /// Over a limit; `headline` is the exceeded counter that resets last.
    Limited {
        headline: RateLimitCounterState,
        retry_after_seconds: u64,
    },
}

/// Whether the counters allow the request (Rack::Attack refuses once a count
/// passes its limit).
pub fn rate_limit_verdict(states: &[RateLimitCounterState], now_ms: u64) -> RateLimitVerdict {
    if let Some(latest) = states
        .iter()
        .filter(|state| state.is_over())
        .max_by_key(|state| state.reset_at_ms)
    {
        return RateLimitVerdict::Limited {
            headline: latest.clone(),
            retry_after_seconds: latest
                .reset_at_ms
                .saturating_sub(now_ms)
                .div_ceil(1000)
                .max(1),
        };
    }
    RateLimitVerdict::Allowed {
        headline: states.iter().min_by_key(|state| state.remaining()).cloned(),
    }
}

/// The key rate limits count an address under, as Mastodon's
/// `throttleable_remote_ip` does: an IPv4 address (or the IPv4 address an
/// IPv4-mapped IPv6 address maps) is itself, an IPv6 address is its `/64`,
/// since one host is routinely handed a whole `/64`. A malformed address has
/// no key.
pub fn ip_throttle_key(raw: &str) -> Option<String> {
    match raw.trim().parse::<IpAddr>().ok()? {
        IpAddr::V4(v4) => Some(v4.to_string()),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => Some(v4.to_string()),
            None => {
                let network = u128::from(v6) & !((1u128 << 64) - 1);
                Some(format!("{}/64", std::net::Ipv6Addr::from(network)))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-29T00:00:00Z: on every window boundary the tests use.
    const START_MS: u64 = 1_793_232_000_000;

    fn request<'a>(
        method: &'a str,
        path: &'a str,
        ip: Option<&'a str>,
        caller: &'a RateLimitCaller,
    ) -> RateLimitRequest<'a> {
        RateLimitRequest {
            method,
            path,
            query: "",
            local_domain: "cfwdon.test",
            ip,
            caller,
        }
    }

    /// Each check as its subject key and counter names.
    fn summary(checks: &[RateLimitCheck]) -> Vec<(String, Vec<String>)> {
        checks
            .iter()
            .map(|check| {
                (
                    check.subject.key(),
                    check.hits.iter().map(|hit| hit.name.clone()).collect(),
                )
            })
            .collect()
    }

    fn checks_for(
        method: &str,
        path: &str,
        caller: &RateLimitCaller,
    ) -> Vec<(String, Vec<String>)> {
        summary(&rate_limit_checks(
            &RateLimitPolicy::mastodon_defaults(),
            &request(method, path, Some("192.0.2.1"), caller),
        ))
    }

    fn account() -> RateLimitCaller {
        RateLimitCaller::Account {
            account_id: "acct-1".to_owned(),
            token_id: "tok-1".to_owned(),
        }
    }

    fn named(subject: &str, names: &[&str]) -> (String, Vec<String>) {
        (
            subject.to_owned(),
            names.iter().map(|name| (*name).to_owned()).collect(),
        )
    }

    #[test]
    fn unauthenticated_api_requests_count_per_address_only_when_one_is_known() {
        let anonymous = RateLimitCaller::Anonymous;
        assert_eq!(
            checks_for("GET", "/api/v1/timelines/public", &anonymous),
            vec![named("ip:192.0.2.1", &["api_ip"])]
        );
        let policy = RateLimitPolicy::mastodon_defaults();
        let without_ip = request("GET", "/api/v1/timelines/public", None, &anonymous);
        assert!(rate_limit_checks(&policy, &without_ip).is_empty());
    }

    #[test]
    fn authenticated_api_requests_count_per_token_and_account_not_per_address() {
        assert_eq!(
            checks_for("GET", "/api/v1/timelines/home", &account()),
            vec![named("account:acct-1", &["api_token:tok-1", "api_account"])]
        );
        let app = RateLimitCaller::Application {
            token_id: "app-tok".to_owned(),
        };
        assert_eq!(
            checks_for("GET", "/api/v1/instance", &app),
            vec![named("token:app-tok", &["api_token"])]
        );
    }

    #[test]
    fn account_action_families_follow_mastodons_rate_limiter() {
        let cases = [
            ("/api/v1/media", "media"),
            ("/api/v2/media/", "media"),
            ("/api/v1/statuses", "statuses"),
            ("/api/v1/statuses/123/reblog", "reblogs"),
            ("/api/v1/accounts/9/follow", "follows"),
            ("/api/v1/reports", "reports"),
        ];
        for (path, family) in cases {
            assert_eq!(
                checks_for("POST", path, &account()),
                vec![named(
                    "account:acct-1",
                    &["api_token:tok-1", "api_account", family]
                )],
                "{path}"
            );
        }
        assert_eq!(
            checks_for("GET", "/api/v1/statuses/123/reblog", &account()),
            vec![named("account:acct-1", &["api_token:tok-1", "api_account"])]
        );
        assert_eq!(
            checks_for("POST", "/api/v1/statuses/123/favourite", &account()),
            vec![named("account:acct-1", &["api_token:tok-1", "api_account"])]
        );
    }

    #[test]
    fn sign_in_oauth_and_federation_buckets_key_on_the_address() {
        let anonymous = RateLimitCaller::Anonymous;
        let cases = [
            ("POST", "/api/v1/apps", vec!["api_ip", "app_registration"]),
            ("POST", "/oauth/token", vec!["oauth_token"]),
            ("GET", "/oauth/authorize", vec!["sign_in"]),
            ("POST", "/oauth/authorize", vec!["sign_in"]),
            ("GET", "/oauth/auth0/callback", vec!["sign_in"]),
            ("GET", "/.well-known/webfinger", vec!["federation_fetch"]),
            ("HEAD", "/nodeinfo/2.1", vec!["federation_fetch"]),
            ("GET", "/users/alice", vec!["federation_fetch"]),
            ("GET", "/users/alice/statuses/1", vec!["federation_fetch"]),
            ("POST", "/inbox", vec!["inbox_ip"]),
            ("POST", "/users/alice/inbox", vec!["inbox_ip"]),
            ("GET", "/actor", vec!["federation_fetch"]),
            ("GET", "/actor/outbox", vec!["federation_fetch"]),
            ("POST", "/actor/inbox", vec!["inbox_ip"]),
        ];
        for (method, path, buckets) in cases {
            assert_eq!(
                checks_for(method, path, &anonymous),
                vec![named("ip:192.0.2.1", &buckets)],
                "{method} {path}"
            );
        }
    }

    #[test]
    fn streaming_web_pages_media_and_internal_routes_are_not_counted() {
        let anonymous = RateLimitCaller::Anonymous;
        for (method, path) in [
            ("GET", "/api/v1/streaming"),
            ("GET", "/api/v1/streaming/user"),
            ("GET", "/app/home"),
            ("GET", "/media/abc"),
            ("GET", "/"),
            ("GET", "/healthz"),
            ("POST", "/internal/outbox/process"),
            ("POST", "/users/alice/outbox"),
        ] {
            assert!(
                checks_for(method, path, &anonymous).is_empty(),
                "{method} {path}"
            );
        }
        assert!(checks_for("GET", "/api/v1/streaming", &account()).is_empty());
    }

    #[test]
    fn overrides_lift_a_bucket_or_replace_its_numbers() {
        let policy = RateLimitPolicy::with_overrides_json(
            r#"{"api_ip": {"limit": 600, "period": 60}, "inbox_domain": false}"#,
        )
        .expect("valid overrides");
        assert_eq!(
            policy.rule(RateLimitBucket::ApiIp),
            RateLimitRule::Throttle {
                limit: 600,
                period_seconds: 60
            }
        );
        assert!(inbox_domain_check(&policy, "remote.example").is_none());
        let anonymous = RateLimitCaller::Anonymous;
        let checks = rate_limit_checks(
            &policy,
            &request("GET", "/api/v1/instance", Some("192.0.2.1"), &anonymous),
        );
        assert_eq!(
            checks[0].hits,
            vec![RateLimitHit {
                name: "api_ip".to_owned(),
                limit: 600,
                period_ms: 60_000,
                counting: RateLimitCounting::LocalFirst,
            }]
        );
        assert_eq!(
            policy.rule(RateLimitBucket::Follows),
            RateLimitPolicy::mastodon_defaults().rule(RateLimitBucket::Follows)
        );
    }

    #[test]
    fn overrides_refuse_unknown_buckets_and_malformed_rules() {
        assert_eq!(
            RateLimitPolicy::with_overrides_json(r#"{"api_ipp": false}"#),
            Err(RateLimitPolicyError::UnknownBucket("api_ipp".to_owned()))
        );
        for raw in [
            r#"{"api_ip": {"limit": 0, "period": 60}}"#,
            r#"{"api_ip": {"limit": 5}}"#,
            r#"{"api_ip": {"limit": 5, "period": 60, "extra": 1}}"#,
            r#"{"api_ip": true}"#,
        ] {
            assert_eq!(
                RateLimitPolicy::with_overrides_json(raw),
                Err(RateLimitPolicyError::MalformedRule("api_ip".to_owned())),
                "{raw}"
            );
        }
        assert_eq!(
            RateLimitPolicy::with_overrides_json("[]"),
            Err(RateLimitPolicyError::NotAnObject)
        );
        assert_eq!(
            RateLimitPolicy::with_overrides_json("not json"),
            Err(RateLimitPolicyError::NotAnObject)
        );
    }

    fn lookup_request<'a>(
        path: &'a str,
        query: &'a str,
        caller: &'a RateLimitCaller,
    ) -> RateLimitRequest<'a> {
        RateLimitRequest {
            query,
            ..request("GET", path, Some("192.0.2.1"), caller)
        }
    }

    fn lookup_summary(
        path: &str,
        query: &str,
        caller: &RateLimitCaller,
    ) -> Vec<(String, Vec<String>)> {
        summary(&rate_limit_checks(
            &RateLimitPolicy::mastodon_defaults(),
            &lookup_request(path, query, caller),
        ))
    }

    #[test]
    fn remote_lookups_count_per_account_and_per_address_when_anonymous() {
        for (path, query) in [
            ("/api/v2/search", "q=alice%40example.com&resolve=true"),
            ("/api/v1/accounts/search", "q=a&resolve=1"),
            ("/api/v1/accounts/lookup", "acct=alice@example.com"),
            ("/api/v1/accounts/lookup", "acct=%40Alice%40Example.com"),
        ] {
            assert_eq!(
                lookup_summary(path, query, &account()),
                vec![named(
                    "account:acct-1",
                    &["api_token:tok-1", "api_account", "remote_lookup"]
                )],
                "{path}?{query}"
            );
            assert_eq!(
                lookup_summary(path, query, &RateLimitCaller::Anonymous),
                vec![named("ip:192.0.2.1", &["api_ip", "remote_lookup"])],
                "{path}?{query}"
            );
        }
    }

    #[test]
    fn local_and_non_resolving_lookups_stay_on_the_general_limits() {
        for (path, query) in [
            ("/api/v2/search", "q=alice"),
            ("/api/v2/search", "q=alice&resolve=false"),
            ("/api/v1/accounts/search", "q=alice@example.com"),
            ("/api/v1/accounts/lookup", "acct=alice"),
            ("/api/v1/accounts/lookup", "acct=alice@CFWDON.test"),
            ("/api/v1/accounts/lookup", ""),
        ] {
            let checks = lookup_summary(path, query, &account());
            assert_eq!(
                checks,
                vec![named("account:acct-1", &["api_token:tok-1", "api_account"])],
                "{path}?{query}"
            );
        }
    }

    #[test]
    fn percent_decoding_handles_malformed_escapes() {
        assert_eq!(percent_decode("a%40b+c"), "a@b c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn defaults_are_mastodons_documented_numbers() {
        let policy = RateLimitPolicy::mastodon_defaults();
        let expected = [
            ("api_ip", 300, 300),
            ("api_token", 300, 300),
            ("api_account", 1500, 300),
            ("media", 30, 1800),
            ("statuses", 300, 10_800),
            ("reblogs", 300, 10_800),
            ("follows", 400, 86_400),
            ("reports", 400, 86_400),
            ("sign_in", 25, 300),
            ("app_registration", 5, 600),
            ("oauth_token", 25, 300),
            ("federation_fetch", 1500, 300),
            ("inbox_ip", 1500, 300),
            ("inbox_domain", 3000, 300),
            ("remote_lookup", 60, 300),
        ];
        assert_eq!(expected.len(), RateLimitBucket::ALL.len());
        for (name, limit, period_seconds) in expected {
            let bucket = RateLimitBucket::from_name(name).expect(name);
            assert_eq!(bucket.name(), name);
            assert_eq!(
                policy.rule(bucket),
                RateLimitRule::Throttle {
                    limit,
                    period_seconds
                },
                "{name}"
            );
        }
    }

    #[test]
    fn inbox_domain_check_keys_on_the_sending_server_case_insensitively() {
        let check = inbox_domain_check(&RateLimitPolicy::mastodon_defaults(), "Remote.Example")
            .expect("throttled by default");
        assert_eq!(check.subject.key(), "domain:remote.example");
        assert_eq!(check.hits[0].name, "inbox_domain");
    }

    fn hit(name: &str, limit: u32, period_ms: u64) -> RateLimitHit {
        RateLimitHit {
            name: name.to_owned(),
            limit,
            period_ms,
            counting: RateLimitCounting::LocalFirst,
        }
    }

    fn once(hit: RateLimitHit) -> RateLimitIncrement {
        RateLimitIncrement { hit, by: 1 }
    }

    #[test]
    fn counts_within_an_epoch_aligned_window_and_restarts_in_the_next() {
        let mut table = RateLimitCounterTable::new();
        let five_minutes = hit("api_ip", 2, 300_000);
        let first = count_hits(&mut table, &[once(five_minutes.clone())], START_MS + 10);
        assert_eq!(first[0].count, 1);
        assert_eq!(first[0].reset_at_ms, START_MS + 300_000);
        let second = count_hits(
            &mut table,
            &[once(five_minutes.clone())],
            START_MS + 299_999,
        );
        assert_eq!(second[0].count, 2);
        let next_window = count_hits(
            &mut table,
            &[once(five_minutes.clone())],
            START_MS + 300_000,
        );
        assert_eq!(next_window[0].count, 1);
        assert_eq!(next_window[0].reset_at_ms, START_MS + 600_000);
    }

    #[test]
    fn keeps_counters_apart_and_drops_windows_that_ended() {
        let mut table = RateLimitCounterTable::new();
        count_hits(
            &mut table,
            &[
                once(hit("api_account", 10, 300_000)),
                once(hit("follows", 10, 86_400_000)),
            ],
            START_MS,
        );
        assert_eq!(table_expires_at_ms(&table), Some(START_MS + 86_400_000));
        let states = count_hits(
            &mut table,
            &[once(hit("follows", 10, 86_400_000))],
            START_MS + 400_000,
        );
        assert_eq!(states[0].count, 2);
        assert!(!table.contains_key("api_account"));
        count_hits(&mut table, &[], START_MS + 86_400_000);
        assert!(table.is_empty());
        assert_eq!(table_expires_at_ms(&table), None);
    }

    #[test]
    fn restarts_a_counter_whose_window_length_changed() {
        let mut table = RateLimitCounterTable::new();
        count_hits(&mut table, &[once(hit("api_ip", 10, 300_000))], START_MS);
        let states = count_hits(&mut table, &[once(hit("api_ip", 10, 60_000))], START_MS + 1);
        assert_eq!(states[0].count, 1);
    }

    /// Isolates counting one subject's hits against a shared counter table,
    /// as the worker and the `RateLimitCounter` Durable Object do.
    struct Deployment {
        isolates: Vec<Vec<LocalCounter>>,
        shared: RateLimitCounterTable,
        asks: usize,
    }

    impl Deployment {
        fn new(isolates: usize) -> Self {
            Self {
                isolates: vec![Vec::new(); isolates],
                shared: RateLimitCounterTable::new(),
                asks: 0,
            }
        }

        /// One request through `isolate`: whether it was allowed.
        fn request(&mut self, isolate: usize, hits: &[RateLimitHit], now_ms: u64) -> bool {
            let counters = &mut self.isolates[isolate];
            counters.resize(hits.len(), LocalCounter::default());
            let states = match plan_hits(counters, hits, now_ms) {
                LocalPlan::Answered(states) => states,
                LocalPlan::Ask(increments) => {
                    self.asks += 1;
                    let states = count_hits(&mut self.shared, &increments, now_ms);
                    for (counter, state) in counters.iter_mut().zip(&states) {
                        counter.record_shared(state);
                    }
                    states
                }
            };
            matches!(
                rate_limit_verdict(&states, now_ms),
                RateLimitVerdict::Allowed { .. }
            )
        }
    }

    #[test]
    fn counts_volume_buckets_locally_until_a_quarter_of_the_limit() {
        let api = [hit("api_ip", 300, 300_000)];
        let mut counters = vec![LocalCounter::default()];
        for expected in 1..=75 {
            match plan_hits(&mut counters, &api, START_MS) {
                LocalPlan::Answered(states) => {
                    assert_eq!(
                        states,
                        vec![state("api_ip", 300, expected, START_MS + 300_000)]
                    );
                }
                LocalPlan::Ask(_) => panic!("hit {expected} asked the shared counter"),
            }
        }
        assert_eq!(
            plan_hits(&mut counters, &api, START_MS),
            LocalPlan::Ask(vec![RateLimitIncrement {
                hit: api[0].clone(),
                by: 76
            }])
        );
    }

    #[test]
    fn exact_buckets_and_mixed_checks_ask_on_every_hit() {
        let policy = RateLimitPolicy::mastodon_defaults();
        let caller = RateLimitCaller::Account {
            account_id: "a".to_owned(),
            token_id: "t".to_owned(),
        };
        let post = rate_limit_checks(&policy, &request("POST", "/api/v1/statuses", None, &caller));
        let hits = &post[0].hits;
        let mut counters = vec![LocalCounter::default(); hits.len()];
        let LocalPlan::Ask(increments) = plan_hits(&mut counters, hits, START_MS) else {
            panic!("a status post was counted locally");
        };
        assert!(increments.iter().all(|increment| increment.by == 1));
        let read = rate_limit_checks(
            &policy,
            &request("GET", "/api/v1/timelines/home", None, &caller),
        );
        assert!(matches!(
            plan_hits(&mut counters[..2], &read[0].hits, START_MS),
            LocalPlan::Answered(_)
        ));
    }

    #[test]
    fn a_client_at_half_its_limit_asks_twice_a_window() {
        let api = [hit("api_token:t", 300, 300_000)];
        let mut deployment = Deployment::new(1);
        for request in 0..150 {
            assert!(deployment.request(0, &api, START_MS + request));
        }
        // At 76 (past a quarter of 300), then at 133 (past 76 and a quarter of the 224 left).
        assert_eq!(deployment.asks, 2);
        assert_eq!(deployment.shared["api_token:t"].count, 133);
    }

    #[test]
    fn a_flood_from_one_client_is_limited_at_the_limit() {
        let api = [hit("api_ip", 300, 300_000)];
        let mut deployment = Deployment::new(1);
        let allowed = (0..2000)
            .filter(|request| deployment.request(0, &api, START_MS + request))
            .count();
        assert_eq!(allowed, 300);
        assert!(deployment.asks < 50, "{} asks", deployment.asks);
        assert!(deployment.request(0, &api, START_MS + 300_000));
    }

    #[test]
    fn a_flood_spread_over_isolates_is_still_limited() {
        let api = [hit("inbox_ip", 1500, 300_000)];
        let isolates = 4;
        let mut deployment = Deployment::new(isolates);
        let allowed = (0..20_000)
            .filter(|request| {
                deployment.request(request % isolates, &api, START_MS + *request as u64)
            })
            .count();
        assert!(allowed >= 1500, "{allowed}");
        assert!(allowed <= 1500 + (isolates - 1) * 1500 / 4, "{allowed}");
    }

    fn state(name: &str, limit: u32, count: u32, reset_at_ms: u64) -> RateLimitCounterState {
        RateLimitCounterState {
            name: name.to_owned(),
            limit,
            count,
            reset_at_ms,
        }
    }

    #[test]
    fn allows_up_to_the_limit_and_names_the_counter_with_the_least_left() {
        let verdict = rate_limit_verdict(
            &[
                state("api_account", 1500, 10, START_MS + 300_000),
                state("api_token:t", 300, 300, START_MS + 300_000),
            ],
            START_MS,
        );
        assert_eq!(
            verdict,
            RateLimitVerdict::Allowed {
                headline: Some(state("api_token:t", 300, 300, START_MS + 300_000))
            }
        );
        assert_eq!(
            rate_limit_verdict(&[], START_MS),
            RateLimitVerdict::Allowed { headline: None }
        );
    }

    #[test]
    fn refuses_past_the_limit_until_the_exceeded_counter_that_resets_last() {
        let short = state("api_account", 1, 2, START_MS + 300_000);
        let long = state("follows", 1, 2, START_MS + 86_400_000);
        let verdict = rate_limit_verdict(
            &[
                short,
                long.clone(),
                state("api_token:t", 300, 1, START_MS + 300_000),
            ],
            START_MS + 500,
        );
        assert_eq!(
            verdict,
            RateLimitVerdict::Limited {
                headline: long,
                retry_after_seconds: 86_400,
            }
        );
        let almost_over = rate_limit_verdict(
            &[state("api_ip", 1, 2, START_MS + 300_000)],
            START_MS + 299_999,
        );
        assert!(matches!(
            almost_over,
            RateLimitVerdict::Limited {
                retry_after_seconds: 1,
                ..
            }
        ));
    }

    #[test]
    fn ip_throttle_key_is_the_ipv4_address_or_the_ipv6_slash_64() {
        assert_eq!(ip_throttle_key("192.0.2.7"), Some("192.0.2.7".to_owned()));
        assert_eq!(
            ip_throttle_key("::ffff:192.0.2.7"),
            Some("192.0.2.7".to_owned())
        );
        assert_eq!(
            ip_throttle_key("2001:db8:1:2:3:4:5:6"),
            Some("2001:db8:1:2::/64".to_owned())
        );
        assert_eq!(
            ip_throttle_key("2001:db8:1:2:ffff::1"),
            ip_throttle_key("2001:db8:1:2::9")
        );
        assert_eq!(ip_throttle_key("not an address"), None);
    }
}
