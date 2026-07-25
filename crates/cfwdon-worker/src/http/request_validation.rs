use crate::{RemoteActorProfile, parse_remote_http_url, sha256_http_digest};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cfwdon_domain::{
    activitypub_date_within_skew, activitypub_host_header_matches_url_host,
    activitypub_key_id_matches_actor, activitypub_signature_lists_required_headers,
    cached_remote_actor_key_matches,
};
use time::{
    Date, Month, OffsetDateTime, PrimitiveDateTime, Time,
    format_description::well_known::{Rfc2822, Rfc3339},
};
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
    cached_remote_actor_key_matches(
        key_id_matches_actor(key_id, actor_uri, &remote_actor.actor_uri),
        &remote_actor.public_key_id,
        key_id,
    )
}

pub(crate) fn extract_activity_actor_uri(activity: &serde_json::Value) -> Result<String> {
    crate::activity_object_id(activity.get("actor"))
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

fn parse_date_with_format(value: &str, format: &str) -> Option<f64> {
    let description = time::format_description::parse(format).ok()?;
    let parsed = PrimitiveDateTime::parse(value, &description).ok()?;
    Some(parsed.assume_utc().unix_timestamp_nanos() as f64 / 1_000_000.0)
}

fn parsed_date_ms(date: Date, time: Time) -> f64 {
    PrimitiveDateTime::new(date, time)
        .assume_utc()
        .unix_timestamp_nanos() as f64
        / 1_000_000.0
}

fn parse_month(value: &str) -> Option<Month> {
    let month = match value {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    Month::try_from(month).ok()
}

fn parse_hms(value: &str) -> Option<Time> {
    let mut parts = value.split(':');
    let hour = parts.next()?.parse().ok()?;
    let minute = parts.next()?.parse().ok()?;
    let second = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Time::from_hms(hour, minute, second).ok()
}

fn parse_rfc850_date_ms(value: &str) -> Option<f64> {
    let (_, rest) = value.split_once(", ")?;
    let mut parts = rest.split_whitespace();
    let date = parts.next()?;
    let time = parse_hms(parts.next()?)?;
    if !parts.next()?.eq_ignore_ascii_case("GMT") || parts.next().is_some() {
        return None;
    }

    let mut date_parts = date.split('-');
    let day = date_parts.next()?.parse().ok()?;
    let month = parse_month(date_parts.next()?)?;
    let year = date_parts.next()?.parse::<i32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let year = if year >= 70 { 1900 + year } else { 2000 + year };
    Some(parsed_date_ms(
        Date::from_calendar_date(year, month, day).ok()?,
        time,
    ))
}

fn parse_asctime_date_ms(value: &str) -> Option<f64> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 5 {
        return None;
    }
    let month = parse_month(parts[1])?;
    let day = parts[2].parse().ok()?;
    let time = parse_hms(parts[3])?;
    let year = parts[4].parse().ok()?;
    Some(parsed_date_ms(
        Date::from_calendar_date(year, month, day).ok()?,
        time,
    ))
}

#[cfg(target_arch = "wasm32")]
fn parse_js_date_ms(value: &str) -> Option<f64> {
    let parsed = js_sys::Date::parse(value);
    (!parsed.is_nan()).then_some(parsed)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_js_date_ms(_value: &str) -> Option<f64> {
    None
}

pub(crate) fn parse_activitypub_request_date_ms(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    OffsetDateTime::parse(trimmed, &Rfc2822)
        .or_else(|_| OffsetDateTime::parse(trimmed, &Rfc3339))
        .map(|parsed| parsed.unix_timestamp_nanos() as f64 / 1_000_000.0)
        .ok()
        .or_else(|| parse_rfc850_date_ms(trimmed))
        .or_else(|| {
            parse_date_with_format(
                trimmed,
                "[weekday repr:short] [month repr:short] [day padding:space] [hour]:[minute]:[second] [year]",
            )
        })
        .or_else(|| parse_asctime_date_ms(trimmed))
        .or_else(|| parse_js_date_ms(trimmed))
}

pub(crate) fn validate_request_date(headers: &Headers) -> Result<()> {
    let date = headers
        .get("Date")?
        .ok_or_else(|| Error::RustError("missing Date header".to_owned()))?;
    let parsed = parse_activitypub_request_date_ms(&date)
        .ok_or_else(|| Error::RustError("invalid Date header".to_owned()))?;

    if !activitypub_date_within_skew(parsed, js_sys::Date::now()) {
        return Err(Error::RustError(
            "Date header outside allowed skew".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) fn validate_activitypub_signature_headers(
    signature: &ParsedSignatureHeader,
) -> Result<()> {
    if activitypub_signature_lists_required_headers(&signature.headers) {
        return Ok(());
    }
    for required_header in cfwdon_domain::ACTIVITYPUB_REQUIRED_SIGNED_HEADERS {
        if !signature
            .headers
            .iter()
            .any(|header| header == required_header)
        {
            return Err(Error::RustError(format!(
                "Signature missing required signed header {required_header}"
            )));
        }
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

/// When `host` is present in the signed header list, require it to match the
/// request URL host. Peers that omit `host` remain accepted for interop.
pub(crate) fn validate_signed_host_header(
    req: &Request,
    headers: &Headers,
    signature: &ParsedSignatureHeader,
) -> Result<()> {
    if !signature.headers.iter().any(|header| header == "host") {
        return Ok(());
    }
    let host_header = headers
        .get("host")?
        .ok_or_else(|| Error::RustError("missing signed header host".to_owned()))?;
    let url = parse_remote_http_url(req.url()?.as_str())?;
    let url_host = url
        .host_str()
        .ok_or_else(|| Error::RustError("request URL is missing host".to_owned()))?;
    if !activitypub_host_header_matches_url_host(&host_header, url_host) {
        return Err(Error::RustError(
            "signed Host header does not match request host".to_owned(),
        ));
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

/// Reconstruct a draft-cavage signing string from ordered header names and values.
/// Used by unit tests and outbound delivery to keep header order consistent.
pub(crate) fn build_signature_signing_string_from_parts(
    method: &str,
    path_and_query: &str,
    signed_headers: &[&str],
    header_values: &[(&str, &str)],
) -> Result<String> {
    let mut lines = Vec::with_capacity(signed_headers.len());
    for header_name in signed_headers {
        let line = if *header_name == "(request-target)" {
            format!(
                "(request-target): {} {}",
                method.to_ascii_lowercase(),
                path_and_query
            )
        } else {
            let value = header_values
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(header_name))
                .map(|(_, value)| *value)
                .ok_or_else(|| Error::RustError(format!("missing signed header {header_name}")))?;
            format!("{header_name}: {value}")
        };
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn key_id_matches_actor(key_id: &str, raw_actor_uri: &str, canonical_actor_uri: &str) -> bool {
    activitypub_key_id_matches_actor(key_id, raw_actor_uri, canonical_actor_uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_activity_actor_uri_accepts_embedded_object() {
        let activity = serde_json::json!({
            "actor": {
                "id": "https://remote.example/users/bob",
                "type": "Person"
            }
        });
        assert_eq!(
            extract_activity_actor_uri(&activity).unwrap(),
            "https://remote.example/users/bob"
        );
    }

    #[test]
    fn extract_activity_actor_uri_accepts_at_id() {
        let activity = serde_json::json!({
            "actor": { "@id": "https://remote.example/users/carol" }
        });
        assert_eq!(
            extract_activity_actor_uri(&activity).unwrap(),
            "https://remote.example/users/carol"
        );
    }

    #[test]
    fn extract_activity_actor_uri_rejects_garbage() {
        let activity = serde_json::json!({
            "actor": { "type": "Person", "name": "no-id" }
        });
        assert!(extract_activity_actor_uri(&activity).is_err());
        assert!(extract_activity_actor_uri(&serde_json::json!({})).is_err());
        assert!(extract_activity_actor_uri(&serde_json::json!({"actor": 1})).is_err());
    }

    #[test]
    fn signing_string_includes_host_and_content_type_in_order() {
        let signed = ["(request-target)", "host", "date", "digest", "content-type"];
        let values = [
            ("host", "social.example"),
            ("date", "Sat, 25 Jul 2026 00:00:00 GMT"),
            ("digest", "SHA-256=abc"),
            ("content-type", "application/activity+json"),
        ];
        let signing_string =
            build_signature_signing_string_from_parts("POST", "/inbox", &signed, &values).unwrap();
        assert_eq!(
            signing_string,
            "(request-target): post /inbox\nhost: social.example\ndate: Sat, 25 Jul 2026 00:00:00 GMT\ndigest: SHA-256=abc\ncontent-type: application/activity+json"
        );
    }
}
