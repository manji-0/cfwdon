use worker::{Response, Result, RouteContext};

const RESPONSE_CACHE_BINDING: &str = "RESPONSE_CACHE";
const ACCOUNT_API_PREFIX: &str = "account_api:v1:";
const ACTOR_JSON_PREFIX: &str = "actor_json:v1:";
const ACTOR_PROFILE_HTML_PREFIX: &str = "actor_profile_html:v1:";
const STATUS_API_PREFIX: &str = "status_api:v1:";
const ACCOUNT_CACHE_TTL_SECONDS: u64 = 300;
const STATUS_API_CACHE_TTL_SECONDS: u64 = 60;
const ACCEPT_VARY_HEADER: &str = "Accept";

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

pub(crate) async fn invalidate_status_api_cache(ctx: &RouteContext<()>, status_id: &str) {
    delete_cache_key(ctx, &status_api_cache_key(status_id)).await;
}

pub(crate) async fn invalidate_account_dynamic_public_cache(
    ctx: &RouteContext<()>,
    account_id: &str,
    username: &str,
) {
    delete_cache_key(ctx, &account_api_cache_key(account_id)).await;
    delete_cache_key(ctx, &actor_profile_html_cache_key(username)).await;
}

pub(crate) async fn invalidate_account_public_cache(
    ctx: &RouteContext<()>,
    account_id: &str,
    username: &str,
) {
    invalidate_account_dynamic_public_cache(ctx, account_id, username).await;
    invalidate_account_actor_document_cache(ctx, username).await;
}

pub(crate) async fn invalidate_account_actor_document_cache(
    ctx: &RouteContext<()>,
    username: &str,
) {
    delete_cache_key(ctx, &actor_json_cache_key(username)).await;
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
    let Some(body) = kv.get(key).cache_ttl(60).text().await? else {
        return Ok(None);
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
    kv.put(key, body)?
        .expiration_ttl(ttl_seconds)
        .execute()
        .await?;
    Ok(())
}

async fn delete_cache_key(ctx: &RouteContext<()>, key: &str) {
    let Ok(kv) = ctx.kv(RESPONSE_CACHE_BINDING) else {
        return;
    };
    let _ = kv.delete(key).await;
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
