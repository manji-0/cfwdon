use super::{Error, Result};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

pub(crate) fn delivery_retry_delay_modifier(attempt: u32) -> &'static str {
    match attempt {
        1 => "+1 minute",
        2 => "+5 minutes",
        3 => "+15 minutes",
        _ => "+60 minutes",
    }
}

pub(crate) fn now_iso_string() -> Result<String> {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .ok_or_else(|| Error::RustError("failed to build ISO timestamp".to_owned()))
}

pub(crate) fn add_seconds_to_iso_string(value: &str, seconds: u64) -> Result<String> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| Error::RustError(format!("invalid ISO timestamp {value}: {error}")))?;
    let seconds = i64::try_from(seconds)
        .map_err(|_| Error::RustError("poll expiration is too large".to_owned()))?;
    (timestamp + Duration::seconds(seconds))
        .format(&Rfc3339)
        .map_err(|error| Error::RustError(format!("failed to format ISO timestamp: {error}")))
}

pub(crate) fn is_iso_timestamp_in_past(value: &str) -> Result<bool> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| Error::RustError(format!("invalid ISO timestamp {value}: {error}")))?;
    let now = OffsetDateTime::parse(&now_iso_string()?, &Rfc3339)
        .map_err(|error| Error::RustError(format!("invalid current ISO timestamp: {error}")))?;
    Ok(timestamp <= now)
}

pub(crate) fn render_status_html(text: &str) -> String {
    let escaped = escape_html(text.trim());
    let paragraphs = escaped
        .split("\n\n")
        .map(|paragraph| paragraph.replace('\n', "<br />"))
        .map(|paragraph| format!("<p>{paragraph}</p>"))
        .collect::<Vec<_>>();

    if paragraphs.is_empty() {
        "<p></p>".to_owned()
    } else {
        paragraphs.join("")
    }
}

pub(crate) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
