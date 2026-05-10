use super::{Error, Response, Result, Serialize};
use worker::ResponseBody;

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

pub(crate) fn cache_public_response(
    mut response: Response,
    max_age_seconds: u32,
) -> Result<Response> {
    response.headers_mut().set(
        "Cache-Control",
        &format!("public, max-age={max_age_seconds}, stale-while-revalidate={max_age_seconds}"),
    )?;
    Ok(response)
}
