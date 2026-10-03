//! Programmatic Workers Cache API helpers for anonymous public responses.
//!
//! Call sites already gate caching to viewer-independent documents. These
//! helpers store/load/delete by absolute URL keys in `caches.default`.
//! Invalidation uses `cache.delete` (Cache-Tag purge is Enterprise-only).
use crate::deferred::defer;
use crate::identity::instance_base_url;
use crate::response_utils::{
    CACHE_TTL_ACCOUNT_API, CACHE_TTL_ANONYMOUS_TIMELINE_PAGE, CACHE_TTL_FEDERATION,
    CACHE_TTL_REMOTE_ACCOUNT_API, CACHE_TTL_STATUS_API,
};
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use sha2::{Digest, Sha256};
use worker::{Cache, Response, ResponseBody, Result, RouteContext};

pub(crate) async fn cached_account_api_response(
    ctx: &RouteContext<()>,
    account_id: &str,
) -> Result<Option<Response>> {
    let config = load_config(ctx);
    cache_get(&account_api_cache_key(&config, account_id)).await
}

pub(crate) async fn cache_account_api_response(
    ctx: &RouteContext<()>,
    account_id: &str,
    value: &MastodonAccountResponse,
) -> Result<()> {
    let config = load_config(ctx);
    let username = value.username.as_str();
    cache_put_json(
        &account_api_cache_key(&config, account_id),
        value,
        "application/json; charset=utf-8",
        CACHE_TTL_ACCOUNT_API,
        &account_cache_tag(username),
    )
    .await
}

pub(crate) async fn cached_actor_json_response(
    ctx: &RouteContext<()>,
    username: &str,
) -> Result<Option<Response>> {
    let config = load_config(ctx);
    cache_get(&actor_json_cache_key(&config, username)).await
}

pub(crate) async fn cache_actor_json_response(
    ctx: &RouteContext<()>,
    username: &str,
    value: &impl serde::Serialize,
) -> Result<()> {
    let config = load_config(ctx);
    cache_put_json(
        &actor_json_cache_key(&config, username),
        value,
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &account_cache_tag(username),
    )
    .await
}

pub(crate) async fn cached_actor_profile_html_response(
    ctx: &RouteContext<()>,
    username: &str,
) -> Result<Option<Response>> {
    let config = load_config(ctx);
    cache_get(&actor_html_cache_key(&config, username)).await
}

pub(crate) async fn cache_actor_profile_html_response(
    ctx: &RouteContext<()>,
    username: &str,
    html: String,
) -> Result<()> {
    let config = load_config(ctx);
    cache_put_html(
        &actor_html_cache_key(&config, username),
        html,
        CACHE_TTL_FEDERATION,
        &account_cache_tag(username),
    )
    .await
}

/// Remote account responses keyed by actor URI. Serving them skips the origin
/// actor fetch, which otherwise runs on every `/api/v1/accounts/:id` and lookup.
pub(crate) async fn cached_remote_account_api_response(
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
) -> Result<Option<Response>> {
    cache_get(&remote_account_api_cache_key(config, actor_uri)).await
}

pub(crate) async fn cache_remote_account_api_response(
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
    value: &MastodonAccountResponse,
) -> Result<()> {
    cache_put_json(
        &remote_account_api_cache_key(config, actor_uri),
        value,
        "application/json; charset=utf-8",
        CACHE_TTL_REMOTE_ACCOUNT_API,
        &format!("remote-account-{}", value.username),
    )
    .await
}

/// Anonymous timeline pages (with their `Link` header) keyed by request URL.
/// The short TTL bounds staleness for views without a cron-refreshed cache.
pub(crate) async fn cached_anonymous_timeline_page(req_url: &str) -> Result<Option<Response>> {
    cache_get(req_url).await
}

pub(crate) fn cache_anonymous_timeline_page(
    req_url: &str,
    page: &[serde_json::Value],
    link: Option<&str>,
) -> Result<()> {
    let body = serde_json::to_vec(page).map_err(|error| {
        worker::Error::RustError(format!("failed to encode cache body: {error}"))
    })?;
    let mut response = Response::from_body(ResponseBody::Body(body))?;
    response
        .headers_mut()
        .set("Content-Type", "application/json; charset=utf-8")?;
    response.headers_mut().set(
        "Cache-Control",
        &cache_control_max_age(CACHE_TTL_ANONYMOUS_TIMELINE_PAGE),
    )?;
    if let Some(link) = link {
        response.headers_mut().set("Link", link)?;
    }
    defer_cache_put(req_url, response);
    Ok(())
}

/// Viewer-independent instance documents (v1 summary, peers) keyed by path.
pub(crate) async fn cached_instance_document(
    config: &cfwdon_core::AppConfig,
    path: &str,
) -> Result<Option<Response>> {
    cache_get(&format!("{}{path}", instance_base_url(config))).await
}

pub(crate) async fn cache_instance_document(
    config: &cfwdon_core::AppConfig,
    path: &str,
    value: &impl serde::Serialize,
    max_age_seconds: u32,
) -> Result<()> {
    cache_put_json(
        &format!("{}{path}", instance_base_url(config)),
        value,
        "application/json; charset=utf-8",
        max_age_seconds,
        "instance",
    )
    .await
}

pub(crate) async fn cached_status_api_response(
    ctx: &RouteContext<()>,
    status_id: &str,
) -> Result<Option<Response>> {
    let config = load_config(ctx);
    cache_get(&status_api_cache_key(&config, status_id)).await
}

pub(crate) async fn cache_status_api_response(
    ctx: &RouteContext<()>,
    status_id: &str,
    value: &impl serde::Serialize,
) -> Result<()> {
    let config = load_config(ctx);
    cache_put_json(
        &status_api_cache_key(&config, status_id),
        value,
        "application/json; charset=utf-8",
        CACHE_TTL_STATUS_API,
        &status_cache_tag(status_id),
    )
    .await
}

pub(crate) async fn invalidate_status_api_cache(ctx: &RouteContext<()>, status_id: &str) {
    let config = load_config(ctx);
    cache_delete(&status_api_cache_key(&config, status_id)).await;
}

pub(crate) async fn invalidate_account_dynamic_public_cache(
    ctx: &RouteContext<()>,
    account_id: &str,
    username: &str,
) {
    invalidate_account_public_cache(ctx, account_id, username).await;
}

pub(crate) async fn invalidate_account_public_cache(
    ctx: &RouteContext<()>,
    account_id: &str,
    username: &str,
) {
    let config = load_config(ctx);
    // Stays awaited: clients re-read right after a profile write.
    let keys = [
        account_api_cache_key(&config, account_id),
        actor_json_cache_key(&config, username),
        actor_html_cache_key(&config, username),
    ];
    futures_util::future::join_all(keys.iter().map(|key| cache_delete(key))).await;
}

fn account_cache_tag(username: &str) -> String {
    format!("account-{username}")
}

fn status_cache_tag(status_id: &str) -> String {
    format!("status-{status_id}")
}

fn status_api_cache_key(config: &cfwdon_core::AppConfig, status_id: &str) -> String {
    format!(
        "{}/api/v1/statuses/{}",
        instance_base_url(config),
        status_id
    )
}

fn account_api_cache_key(config: &cfwdon_core::AppConfig, account_id: &str) -> String {
    format!(
        "{}/api/v1/accounts/{}",
        instance_base_url(config),
        account_id
    )
}

fn remote_account_api_cache_key(config: &cfwdon_core::AppConfig, actor_uri: &str) -> String {
    format!(
        "{}/cfwdon-cache/remote-accounts/{:x}",
        instance_base_url(config),
        Sha256::digest(actor_uri.as_bytes())
    )
}

fn actor_json_cache_key(config: &cfwdon_core::AppConfig, username: &str) -> String {
    format!("{}/users/{}", instance_base_url(config), username)
}

fn actor_html_cache_key(config: &cfwdon_core::AppConfig, username: &str) -> String {
    // Distinct key from activity+json; same path is Accept-negotiated at the edge.
    format!(
        "{}/users/{}?cfwdon_cache=html",
        instance_base_url(config),
        username
    )
}

fn cache_control_max_age(max_age_seconds: u32) -> String {
    // Cache API put/get do not honor stale-while-revalidate.
    format!("public, max-age={max_age_seconds}")
}

async fn cache_get(key: &str) -> Result<Option<Response>> {
    let Some(mut cached) = Cache::default().get(key, true).await.unwrap_or_default() else {
        return Ok(None);
    };

    // Cache API hits are immutable; rebuild so CORS / other middleware can mutate headers.
    let status = cached.status_code();
    let content_type = cached
        .headers()
        .get("Content-Type")?
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let cache_control = cached.headers().get("Cache-Control")?;
    let cache_tag = cached.headers().get("Cache-Tag")?;
    let vary = cached.headers().get("Vary")?;
    let link = cached.headers().get("Link")?;
    let body = cached.bytes().await?;
    let mut response = Response::from_body(ResponseBody::Body(body))?.with_status(status);
    response.headers_mut().set("Content-Type", &content_type)?;
    if let Some(value) = cache_control {
        response.headers_mut().set("Cache-Control", &value)?;
    }
    if let Some(value) = cache_tag {
        response.headers_mut().set("Cache-Tag", &value)?;
    }
    if let Some(value) = vary {
        response.headers_mut().set("Vary", &value)?;
    }
    if let Some(value) = link {
        response.headers_mut().set("Link", &value)?;
    }
    Ok(Some(response))
}

async fn cache_delete(key: &str) {
    let _ = Cache::default().delete(key, true).await;
}

/// The response does not wait for the cache write; a miss until it lands only
/// costs a recompute.
fn defer_cache_put(key: &str, response: Response) {
    let key = key.to_owned();
    defer(async move {
        let _ = Cache::default().put(key, response).await;
    });
}

async fn cache_put_json(
    key: &str,
    value: &impl serde::Serialize,
    content_type: &str,
    max_age_seconds: u32,
    cache_tag: &str,
) -> Result<()> {
    let body = serde_json::to_vec(value).map_err(|error| {
        worker::Error::RustError(format!("failed to encode cache body: {error}"))
    })?;
    let mut response = Response::from_body(ResponseBody::Body(body))?;
    response.headers_mut().set("Content-Type", content_type)?;
    response
        .headers_mut()
        .set("Cache-Control", &cache_control_max_age(max_age_seconds))?;
    response.headers_mut().set("Cache-Tag", cache_tag)?;
    defer_cache_put(key, response);
    Ok(())
}

async fn cache_put_html(
    key: &str,
    html: String,
    max_age_seconds: u32,
    cache_tag: &str,
) -> Result<()> {
    let mut response = Response::from_body(ResponseBody::Body(html.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response
        .headers_mut()
        .set("Cache-Control", &cache_control_max_age(max_age_seconds))?;
    response.headers_mut().set("Cache-Tag", cache_tag)?;
    response.headers_mut().set("Vary", "Accept")?;
    defer_cache_put(key, response);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        account_api_cache_key, actor_html_cache_key, actor_json_cache_key, cache_control_max_age,
        status_api_cache_key,
    };
    use cfwdon_core::AppConfig;

    fn test_config() -> AppConfig {
        AppConfig {
            instance_domain: "social.example".to_owned(),
            ..AppConfig::default()
        }
    }

    #[test]
    fn cache_keys_are_absolute_and_stable() {
        let config = test_config();
        assert_eq!(
            status_api_cache_key(&config, "status-1"),
            "https://social.example/api/v1/statuses/status-1"
        );
        assert_eq!(
            account_api_cache_key(&config, "abc"),
            "https://social.example/api/v1/accounts/abc"
        );
        assert_eq!(
            actor_json_cache_key(&config, "alice"),
            "https://social.example/users/alice"
        );
        assert_eq!(
            actor_html_cache_key(&config, "alice"),
            "https://social.example/users/alice?cfwdon_cache=html"
        );
    }

    #[test]
    fn cache_control_for_cache_api_omits_stale_while_revalidate() {
        assert_eq!(cache_control_max_age(60), "public, max-age=60");
    }
}
