use worker::{Response, Result, RouteContext};

const RESPONSE_CACHE_BINDING: &str = "RESPONSE_CACHE";
const STATUS_API_PREFIX: &str = "status_api:v1:";
const STATUS_API_CACHE_TTL_SECONDS: u64 = 60;

pub(crate) async fn cached_status_api_response(
    ctx: &RouteContext<()>,
    status_id: &str,
) -> Result<Option<Response>> {
    let Ok(kv) = ctx.kv(RESPONSE_CACHE_BINDING) else {
        return Ok(None);
    };
    let Some(body) = kv
        .get(&status_api_cache_key(status_id))
        .cache_ttl(60)
        .text()
        .await?
    else {
        return Ok(None);
    };
    let mut response = Response::ok(body)?;
    response
        .headers_mut()
        .set("Content-Type", "application/json")?;
    response.headers_mut().set(
        "Cache-Control",
        "public, max-age=30, stale-while-revalidate=60",
    )?;
    response.headers_mut().set("X-Cfwdon-Cache", "kv")?;
    Ok(Some(response))
}

pub(crate) async fn cache_status_api_response(
    ctx: &RouteContext<()>,
    status_id: &str,
    value: &crate::MastodonStatusResponse,
) -> Result<()> {
    let Ok(kv) = ctx.kv(RESPONSE_CACHE_BINDING) else {
        return Ok(());
    };
    let body = serde_json::to_string(value).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize status cache payload: {error}"))
    })?;
    kv.put(&status_api_cache_key(status_id), body)?
        .expiration_ttl(STATUS_API_CACHE_TTL_SECONDS)
        .execute()
        .await?;
    Ok(())
}

pub(crate) async fn invalidate_status_api_cache(ctx: &RouteContext<()>, status_id: &str) {
    let Ok(kv) = ctx.kv(RESPONSE_CACHE_BINDING) else {
        return;
    };
    let _ = kv.delete(&status_api_cache_key(status_id)).await;
}

fn status_api_cache_key(status_id: &str) -> String {
    format!("{STATUS_API_PREFIX}{status_id}")
}

#[cfg(test)]
mod tests {
    use super::status_api_cache_key;

    #[test]
    fn status_api_cache_key_uses_stable_prefix() {
        assert_eq!(status_api_cache_key("123"), "status_api:v1:123");
    }
}
