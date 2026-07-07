use crate::{Error, Response, Result, Serialize};
use worker::ResponseBody;

pub(crate) const CACHE_TTL_HEALTH: u32 = 30;
pub(crate) const CACHE_TTL_INSTANCE_SUMMARY: u32 = 60;
pub(crate) const CACHE_TTL_FEDERATION: u32 = 60;
pub(crate) const CACHE_TTL_TRENDS: u32 = 60;
pub(crate) const CACHE_TTL_STATIC_METADATA: u32 = 300;
pub(crate) const CACHE_TTL_OAUTH_DISCOVERY: u32 = 3600;
pub(crate) const CACHE_TTL_OEMBED: u32 = 3600;

pub(crate) fn json_response<T>(
    value: &T,
    content_type: &str,
    extra_headers: &[(&str, &str)],
) -> Result<Response>
where
    T: Serialize,
{
    let body = serde_json::to_string(value)
        .map_err(|error| Error::RustError(format!("failed to serialize response: {error}")))?;
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    response.headers_mut().set("Content-Type", content_type)?;

    for (name, value) in extra_headers {
        response.headers_mut().set(name, value)?;
    }

    Ok(response)
}

pub(crate) fn cache_public_response(response: Response, max_age_seconds: u32) -> Result<Response> {
    cache_public_response_with_options(response, max_age_seconds, None, &[])
}

pub(crate) fn cache_public_response_with_options(
    mut response: Response,
    max_age_seconds: u32,
    stale_while_revalidate_seconds: Option<u32>,
    extra_headers: &[(&str, &str)],
) -> Result<Response> {
    let stale_while_revalidate = stale_while_revalidate_seconds
        .unwrap_or_else(|| default_stale_while_revalidate(max_age_seconds));
    response.headers_mut().set(
        "Cache-Control",
        &cache_control_header(max_age_seconds, stale_while_revalidate),
    )?;

    for (name, value) in extra_headers {
        response.headers_mut().set(name, value)?;
    }

    Ok(response)
}

pub(crate) fn cache_public_json_response<T>(
    value: &T,
    content_type: &str,
    max_age_seconds: u32,
    extra_headers: &[(&str, &str)],
) -> Result<Response>
where
    T: Serialize,
{
    cache_public_response(
        json_response(value, content_type, extra_headers)?,
        max_age_seconds,
    )
}

fn default_stale_while_revalidate(max_age_seconds: u32) -> u32 {
    max_age_seconds.saturating_mul(5).min(3600)
}

fn cache_control_header(max_age_seconds: u32, stale_while_revalidate_seconds: u32) -> String {
    format!(
        "public, max-age={max_age_seconds}, stale-while-revalidate={stale_while_revalidate_seconds}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_control_header_sets_stale_while_revalidate() {
        assert_eq!(
            cache_control_header(60, default_stale_while_revalidate(60)),
            "public, max-age=60, stale-while-revalidate=300"
        );
    }

    #[test]
    fn cache_control_header_caps_stale_while_revalidate() {
        assert_eq!(
            cache_control_header(900, default_stale_while_revalidate(900)),
            "public, max-age=900, stale-while-revalidate=3600"
        );
    }
}
