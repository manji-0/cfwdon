use crate::{log_observed_operation, observability_started_at_ms};
use std::future::Future;
use worker::{Context, Env, Response, Result, RouteContext};

const RESPONSE_CACHE_BINDING: &str = "RESPONSE_CACHE";
const ACCOUNT_API_PREFIX: &str = "account_api:v1:";
const ACTOR_JSON_PREFIX: &str = "actor_json:v1:";
const ACTOR_PROFILE_HTML_PREFIX: &str = "actor_profile_html:v1:";
const STATUS_API_PREFIX: &str = "status_api:v1:";
const ACCOUNT_CACHE_TTL_SECONDS: u64 = 300;
const STATUS_API_CACHE_TTL_SECONDS: u64 = 60;
const ACCEPT_VARY_HEADER: &str = "Accept";

pub(crate) trait BackgroundTaskContext {
    fn schedule_background_task<F>(&self, future: F) -> bool
    where
        F: Future<Output = ()> + 'static;
}

impl BackgroundTaskContext for () {
    fn schedule_background_task<F>(&self, _future: F) -> bool
    where
        F: Future<Output = ()> + 'static,
    {
        false
    }
}

pub(crate) struct FetchEventContext {
    event_context: Context,
}

impl FetchEventContext {
    pub(crate) fn new(event_context: Context) -> Self {
        Self { event_context }
    }
}

impl BackgroundTaskContext for FetchEventContext {
    fn schedule_background_task<F>(&self, future: F) -> bool
    where
        F: Future<Output = ()> + 'static,
    {
        self.event_context.wait_until(future);
        true
    }
}

pub(crate) async fn cached_account_api_response(
    ctx: &RouteContext<()>,
    account_id: &str,
) -> Result<Option<Response>> {
    cached_text_response(
        ctx,
        &account_api_cache_key(account_id),
        "application/json",
        "public, max-age=60, stale-while-revalidate=300",
        None,
    )
    .await
}

pub(crate) async fn cache_account_api_response(
    ctx: &RouteContext<()>,
    account_id: &str,
    value: &crate::MastodonAccountResponse,
) -> Result<()> {
    cache_json_value(
        ctx,
        &account_api_cache_key(account_id),
        value,
        ACCOUNT_CACHE_TTL_SECONDS,
        "account cache payload",
    )
    .await
}

pub(crate) async fn cached_actor_json_response(
    ctx: &RouteContext<()>,
    username: &str,
) -> Result<Option<Response>> {
    cached_text_response(
        ctx,
        &actor_json_cache_key(username),
        "application/activity+json",
        "public, max-age=60, stale-while-revalidate=300",
        Some(ACCEPT_VARY_HEADER),
    )
    .await
}

pub(crate) async fn cache_actor_json_response(
    ctx: &RouteContext<()>,
    username: &str,
    value: &impl serde::Serialize,
) -> Result<()> {
    cache_json_value(
        ctx,
        &actor_json_cache_key(username),
        value,
        ACCOUNT_CACHE_TTL_SECONDS,
        "actor cache payload",
    )
    .await
}

pub(crate) async fn cached_actor_profile_html_response(
    ctx: &RouteContext<()>,
    username: &str,
) -> Result<Option<Response>> {
    cached_text_response(
        ctx,
        &actor_profile_html_cache_key(username),
        "text/html; charset=utf-8",
        "public, max-age=60, stale-while-revalidate=300",
        Some(ACCEPT_VARY_HEADER),
    )
    .await
}

pub(crate) async fn cache_actor_profile_html_response(
    ctx: &RouteContext<()>,
    username: &str,
    html: String,
) -> Result<()> {
    cache_text(
        ctx,
        &actor_profile_html_cache_key(username),
        html,
        ACCOUNT_CACHE_TTL_SECONDS,
    )
    .await
}

pub(crate) async fn cached_status_api_response(
    ctx: &RouteContext<()>,
    status_id: &str,
) -> Result<Option<Response>> {
    cached_text_response(
        ctx,
        &status_api_cache_key(status_id),
        "application/json",
        "public, max-age=30, stale-while-revalidate=60",
        None,
    )
    .await
}

pub(crate) async fn cache_status_api_response(
    ctx: &RouteContext<()>,
    status_id: &str,
    value: &crate::MastodonStatusResponse,
) -> Result<()> {
    cache_json_value(
        ctx,
        &status_api_cache_key(status_id),
        value,
        STATUS_API_CACHE_TTL_SECONDS,
        "status cache payload",
    )
    .await
}

pub(crate) async fn invalidate_status_api_cache<D>(ctx: &RouteContext<D>, status_id: &str)
where
    D: BackgroundTaskContext,
{
    delete_cache_keys_from_env(ctx.env.clone(), vec![status_api_cache_key(status_id)]).await;
}

pub(crate) async fn invalidate_account_dynamic_public_cache(
    ctx: &RouteContext<impl BackgroundTaskContext>,
    account_id: &str,
    username: &str,
) {
    let keys = vec![
        account_api_cache_key(account_id),
        actor_profile_html_cache_key(username),
    ];
    if ctx
        .data
        .schedule_background_task(delete_cache_keys_from_env(ctx.env.clone(), keys.clone()))
    {
        return;
    }
    delete_cache_keys_from_env(ctx.env.clone(), keys).await;
}

pub(crate) async fn invalidate_account_public_cache(
    ctx: &RouteContext<impl BackgroundTaskContext>,
    account_id: &str,
    username: &str,
) {
    invalidate_account_dynamic_public_cache(ctx, account_id, username).await;
    invalidate_account_actor_document_cache(ctx, username).await;
}

pub(crate) async fn invalidate_account_actor_document_cache(
    ctx: &RouteContext<impl BackgroundTaskContext>,
    username: &str,
) {
    delete_cache_keys_from_env(ctx.env.clone(), vec![actor_json_cache_key(username)]).await;
}

async fn cached_text_response(
    ctx: &RouteContext<()>,
    key: &str,
    content_type: &str,
    cache_control: &str,
    vary: Option<&str>,
) -> Result<Option<Response>> {
    let Ok(kv) = ctx.kv(RESPONSE_CACHE_BINDING) else {
        return Ok(None);
    };
    let started_at_ms = observability_started_at_ms();
    let cached_body = kv.get(key).cache_ttl(60).text().await;
    let body = match cached_body {
        Ok(Some(body)) => {
            log_cache_operation("get", "hit", started_at_ms, key, Some(body.len()));
            body
        }
        Ok(None) => {
            log_cache_operation("get", "miss", started_at_ms, key, None);
            return Ok(None);
        }
        Err(error) => {
            log_cache_operation("get", "error", started_at_ms, key, None);
            return Err(worker::Error::KvError(error));
        }
    };
    let mut response = Response::ok(body)?;
    response.headers_mut().set("Content-Type", content_type)?;
    response.headers_mut().set("Cache-Control", cache_control)?;
    if let Some(vary) = vary {
        response.headers_mut().set("Vary", vary)?;
    }
    response.headers_mut().set("X-Cfwdon-Cache", "kv")?;
    Ok(Some(response))
}

async fn cache_json_value<T: serde::Serialize>(
    ctx: &RouteContext<()>,
    key: &str,
    value: &T,
    ttl_seconds: u64,
    label: &str,
) -> Result<()> {
    let body = serde_json::to_string(value).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize {label}: {error}"))
    })?;
    cache_text(ctx, key, body, ttl_seconds).await
}

async fn cache_text(
    ctx: &RouteContext<()>,
    key: &str,
    body: String,
    ttl_seconds: u64,
) -> Result<()> {
    let Ok(kv) = ctx.kv(RESPONSE_CACHE_BINDING) else {
        return Ok(());
    };
    let bytes = body.len();
    let started_at_ms = observability_started_at_ms();
    let result = kv
        .put(key, body)?
        .expiration_ttl(ttl_seconds)
        .execute()
        .await;
    let outcome = if result.is_ok() { "ok" } else { "error" };
    log_cache_operation("put", outcome, started_at_ms, key, Some(bytes));
    result?;
    Ok(())
}

async fn delete_cache_keys_from_env(env: Env, keys: Vec<String>) {
    let Ok(kv) = env.kv(RESPONSE_CACHE_BINDING) else {
        return;
    };
    let deletes = keys.into_iter().map(|key| {
        let kv = kv.clone();
        async move {
            let started_at_ms = observability_started_at_ms();
            match kv.delete(&key).await {
                Ok(()) => log_cache_operation("delete", "ok", started_at_ms, &key, None),
                Err(_) => log_cache_operation("delete", "error", started_at_ms, &key, None),
            }
        }
    });
    futures_util::future::join_all(deletes).await;
}

fn log_cache_operation(
    operation: &str,
    outcome: &str,
    started_at_ms: f64,
    key: &str,
    bytes: Option<usize>,
) {
    let mut details = serde_json::json!({
        "binding": RESPONSE_CACHE_BINDING,
        "cache_family": cache_key_family(key),
    });
    if let (Some(details), Some(bytes)) = (details.as_object_mut(), bytes) {
        details.insert("bytes".to_owned(), serde_json::json!(bytes));
    }
    log_observed_operation("kv", operation, outcome, started_at_ms, details);
}

fn cache_key_family(key: &str) -> &'static str {
    if key.starts_with(ACCOUNT_API_PREFIX) {
        "account_api"
    } else if key.starts_with(ACTOR_JSON_PREFIX) {
        "actor_json"
    } else if key.starts_with(ACTOR_PROFILE_HTML_PREFIX) {
        "actor_profile_html"
    } else if key.starts_with(STATUS_API_PREFIX) {
        "status_api"
    } else {
        "unknown"
    }
}

fn account_api_cache_key(account_id: &str) -> String {
    format!("{ACCOUNT_API_PREFIX}{account_id}")
}

fn actor_json_cache_key(username: &str) -> String {
    format!("{ACTOR_JSON_PREFIX}{}", username.to_ascii_lowercase())
}

fn actor_profile_html_cache_key(username: &str) -> String {
    format!(
        "{ACTOR_PROFILE_HTML_PREFIX}{}",
        username.to_ascii_lowercase()
    )
}

fn status_api_cache_key(status_id: &str) -> String {
    format!("{STATUS_API_PREFIX}{status_id}")
}

#[cfg(test)]
mod tests {
    use super::{
        account_api_cache_key, actor_json_cache_key, actor_profile_html_cache_key,
        status_api_cache_key,
    };

    #[test]
    fn account_cache_keys_use_stable_prefixes() {
        assert_eq!(account_api_cache_key("123"), "account_api:v1:123");
        assert_eq!(actor_json_cache_key("Alice"), "actor_json:v1:alice");
        assert_eq!(
            actor_profile_html_cache_key("Alice"),
            "actor_profile_html:v1:alice"
        );
    }

    #[test]
    fn status_api_cache_key_uses_stable_prefix() {
        assert_eq!(status_api_cache_key("123"), "status_api:v1:123");
    }
}
