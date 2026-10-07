use super::NotificationEntry;
use crate::time_html::timestamp_to_mastodon_iso8601;

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 14695981039346656037_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

fn created_at_millis(created_at: &str) -> i64 {
    time::OffsetDateTime::parse(
        &timestamp_to_mastodon_iso8601(created_at),
        &time::format_description::well_known::Rfc3339,
    )
    .map(|value| (value.unix_timestamp_nanos() / 1_000_000) as i64)
    .unwrap_or_default()
    .max(0)
}

/// The id clients see for a notification. Notifications are assembled from
/// several tables and keyed internally by strings such as
/// `favourite-remote-…`; Mastodon clients expect snowflake ids instead
/// (creation time in milliseconds shifted left 16 bits, low bits to break
/// ties) that compare numerically in time order. Deriving one from the
/// creation time and the internal key keeps it stable across requests and
/// identical in v1, v2 and streaming payloads.
pub(crate) fn notification_api_id(internal_id: &str, created_at: &str) -> i64 {
    (created_at_millis(created_at) << 16) | (fnv1a64(internal_id.as_bytes()) & 0xffff) as i64
}

/// Ids handed out before snowflake ids were hashes in
/// `[LEGACY_ID_BASE, LEGACY_ID_BASE + LEGACY_ID_SPAN)`. Any snowflake for a
/// notification created after 1974 is above that range, so a value inside it
/// is a legacy id, which clients may still hold as a cursor or marker.
const LEGACY_ID_BASE: u64 = 1_000_000_000_000_000;
const LEGACY_ID_SPAN: u64 = 8_000_000_000_000_000;

/// What identifies a notification: its internal key and creation time. Both
/// rendered entries and not-yet-rendered candidates carry them, so ids,
/// cursors and dismissals work on either.
pub(crate) trait NotificationIdentity {
    fn notification_key(&self) -> &str;
    fn notification_created_at(&self) -> &str;
}

impl NotificationIdentity for NotificationEntry {
    fn notification_key(&self) -> &str {
        &self.id
    }

    fn notification_created_at(&self) -> &str {
        &self.created_at
    }
}

impl<T: NotificationIdentity + ?Sized> NotificationIdentity for &T {
    fn notification_key(&self) -> &str {
        (**self).notification_key()
    }

    fn notification_created_at(&self) -> &str {
        (**self).notification_created_at()
    }
}

fn legacy_notification_api_id<T: NotificationIdentity>(entry: &T) -> i64 {
    let value = LEGACY_ID_BASE + (fnv1a64(entry.notification_key().as_bytes()) % LEGACY_ID_SPAN);
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn is_legacy_notification_api_id(value: i64) -> bool {
    u64::try_from(value)
        .is_ok_and(|value| (LEGACY_ID_BASE..LEGACY_ID_BASE + LEGACY_ID_SPAN).contains(&value))
}

pub(crate) fn notification_api_numeric_id<T: NotificationIdentity>(entry: &T) -> i64 {
    notification_api_id(entry.notification_key(), entry.notification_created_at())
}

pub(crate) fn notification_api_numeric_id_string<T: NotificationIdentity>(entry: &T) -> String {
    notification_api_numeric_id(entry).to_string()
}

/// A cursor or path id names an entry by its API id or, for ids handed out
/// before API ids existed, by its internal key.
pub(crate) fn notification_entry_matches_cursor_id<T: NotificationIdentity>(
    entry: &T,
    cursor_id: &str,
) -> bool {
    if entry.notification_key() == cursor_id {
        return true;
    }
    cursor_id.parse::<i64>().ok().is_some_and(|cursor_numeric| {
        if is_legacy_notification_api_id(cursor_numeric) {
            legacy_notification_api_id(entry) == cursor_numeric
        } else {
            notification_api_numeric_id(entry) == cursor_numeric
        }
    })
}

/// The v1 notification document: the stored value with the API id and the
/// grouped-notifications key Mastodon also exposes on v1 entities.
pub(crate) fn notification_v1_value(entry: &NotificationEntry) -> serde_json::Value {
    let mut value = entry.value.clone();
    value["id"] = serde_json::json!(notification_api_numeric_id_string(entry));
    value["group_key"] = serde_json::json!(super::notification_v2_group_key(
        entry,
        &super::default_grouped_notification_types(),
    ));
    value
}
