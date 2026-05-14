use crate::{RemoteActorProfile, parse_remote_http_url, sha256_http_digest};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use worker::{Error, Headers, Request, Result};

#[derive(Debug)]
pub(crate) struct ParsedSignatureHeader {
    pub(crate) key_id: String,
    pub(crate) headers: Vec<String>,
    pub(crate) signature: Vec<u8>,
}

pub(crate) fn cached_remote_actor_matches_key(
    remote_actor: &RemoteActorProfile,
    key_id: &str,
    actor_uri: &str,
) -> bool {
    if !key_id_matches_actor(key_id, actor_uri, &remote_actor.actor_uri) {
        return false;
    }
    remote_actor.public_key_id.is_empty() || key_id == remote_actor.public_key_id
}

pub(crate) fn extract_activity_actor_uri(activity: &serde_json::Value) -> Result<String> {
    activity
        .get("actor")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::RustError("activity is missing actor".to_owned()))
}

pub(crate) fn parse_signature_header(header: &str) -> Result<ParsedSignatureHeader> {
    let mut key_id = None;
    let mut headers = None;
    let mut signature = None;

    for part in header.split(',') {
        let mut segments = part.trim().splitn(2, '=');
        let Some(name) = segments.next() else {
            continue;
        };
        let Some(raw_value) = segments.next() else {
            continue;
        };
        let value = raw_value.trim().trim_matches('"');

        match name.trim() {
            "keyId" => key_id = Some(value.to_owned()),
            "headers" => {
                headers = Some(
                    value
                        .split_whitespace()
                        .map(|entry| entry.to_ascii_lowercase())
                        .collect::<Vec<_>>(),
                )
            }
            "signature" => {
                signature = Some(STANDARD.decode(value).map_err(|error| {
                    Error::RustError(format!("invalid Signature header encoding: {error}"))
                })?)
            }
            _ => {}
        }
    }

    Ok(ParsedSignatureHeader {
        key_id: key_id.ok_or_else(|| Error::RustError("Signature keyId missing".to_owned()))?,
        headers: headers.ok_or_else(|| Error::RustError("Signature headers missing".to_owned()))?,
        signature: signature
            .ok_or_else(|| Error::RustError("Signature value missing".to_owned()))?,
    })
}

pub(crate) fn validate_request_date(headers: &Headers) -> Result<()> {
    let date = headers
        .get("Date")?
        .ok_or_else(|| Error::RustError("missing Date header".to_owned()))?;
    let parsed = js_sys::Date::parse(&date);
    if parsed.is_nan() {
        return Err(Error::RustError("invalid Date header".to_owned()));
    }

    let skew_ms = (js_sys::Date::now() - parsed).abs();
    if skew_ms > 12.0 * 60.0 * 60.0 * 1000.0 {
        return Err(Error::RustError(
            "Date header outside allowed skew".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) async fn validate_request_digest(headers: &Headers, body: &[u8]) -> Result<()> {
    let digest = headers
        .get("Digest")?
        .ok_or_else(|| Error::RustError("missing Digest header".to_owned()))?;
    let (algorithm, value) = digest
        .split_once('=')
        .ok_or_else(|| Error::RustError("invalid Digest header".to_owned()))?;
    if !algorithm.eq_ignore_ascii_case("sha-256") {
        return Err(Error::RustError("unsupported Digest algorithm".to_owned()));
    }

    let expected = sha256_http_digest(body).await?;
    let expected_value = expected
        .split_once('=')
        .map(|(_, value)| value)
        .unwrap_or_default();
    if value != expected_value {
        return Err(Error::RustError("Digest header mismatch".to_owned()));
    }

    Ok(())
}

pub(crate) fn build_signature_signing_string(
    req: &Request,
    headers: &Headers,
    signature: &ParsedSignatureHeader,
) -> Result<String> {
    let url = parse_remote_http_url(req.url()?.as_str())?;
    let path_and_query = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    let mut lines = Vec::with_capacity(signature.headers.len());

    for header_name in &signature.headers {
        let line = if header_name == "(request-target)" {
            format!(
                "(request-target): {} {}",
                req.method().as_ref().to_ascii_lowercase(),
                path_and_query
            )
        } else {
            let value = headers
                .get(header_name)?
                .ok_or_else(|| Error::RustError(format!("missing signed header {header_name}")))?;
            format!("{header_name}: {value}")
        };
        lines.push(line);
    }

    Ok(lines.join("\n"))
}

fn key_id_matches_actor(key_id: &str, raw_actor_uri: &str, canonical_actor_uri: &str) -> bool {
    let Ok(key_url) = parse_remote_http_url(key_id) else {
        return false;
    };
    let mut key_actor = key_url.clone();
    key_actor.set_fragment(None);

    key_actor.as_str() == raw_actor_uri || key_actor.as_str() == canonical_actor_uri
}
