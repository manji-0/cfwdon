use super::{Error, Result};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

pub(crate) fn now_iso_string() -> Result<String> {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::new_0()
            .to_iso_string()
            .as_string()
            .ok_or_else(|| Error::RustError("failed to build ISO timestamp".to_owned()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| Error::RustError(format!("failed to format ISO timestamp: {error}")))
    }
}

pub(crate) fn now_unix_timestamp() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0).floor() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        OffsetDateTime::now_utc().unix_timestamp()
    }
}

pub(crate) fn subtract_seconds_from_iso_string(value: &str, seconds: i64) -> Result<String> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| Error::RustError(format!("invalid ISO timestamp {value}: {error}")))?;
    (timestamp - Duration::seconds(seconds))
        .format(&Rfc3339)
        .map_err(|error| Error::RustError(format!("failed to format ISO timestamp: {error}")))
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

/// Normalize D1 / SQLite timestamps into ActivityPub-friendly RFC3339 UTC.
///
/// Accepts already-valid RFC3339 values, or `YYYY-MM-DD HH:MM:SS` (SQLite
/// `CURRENT_TIMESTAMP`) treated as UTC.
pub(crate) fn activitypub_datetime_string(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(timestamp) = OffsetDateTime::parse(trimmed, &Rfc3339) {
        return timestamp
            .format(&Rfc3339)
            .unwrap_or_else(|_| trimmed.to_owned());
    }

    // SQLite CURRENT_TIMESTAMP: "2026-05-09 13:40:48"
    if trimmed.len() >= 19 && trimmed.as_bytes().get(10) == Some(&b' ') {
        let candidate = format!("{}T{}Z", &trimmed[..10], &trimmed[11..19]);
        if let Ok(timestamp) = OffsetDateTime::parse(&candidate, &Rfc3339) {
            return timestamp.format(&Rfc3339).unwrap_or(candidate);
        }
    }

    trimmed.to_owned()
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

#[cfg(test)]
mod tests {
    use super::activitypub_datetime_string;

    #[test]
    fn activitypub_datetime_string_normalizes_sqlite_timestamp() {
        assert_eq!(
            activitypub_datetime_string("2026-05-09 13:40:48"),
            "2026-05-09T13:40:48Z"
        );
    }

    #[test]
    fn activitypub_datetime_string_keeps_rfc3339() {
        let normalized = activitypub_datetime_string("2026-05-09T13:40:48.000Z");
        assert!(
            normalized == "2026-05-09T13:40:48.000Z" || normalized == "2026-05-09T13:40:48Z",
            "unexpected normalized timestamp: {normalized}"
        );
    }

    #[test]
    fn activitypub_datetime_string_leaves_empty() {
        assert_eq!(activitypub_datetime_string(""), "");
        assert_eq!(activitypub_datetime_string("   "), "");
    }
}
