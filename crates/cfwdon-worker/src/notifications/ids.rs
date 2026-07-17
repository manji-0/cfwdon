use super::NotificationEntry;

const NOTIFICATION_API_ID_BASE: u64 = 1_000_000_000_000_000;
const NOTIFICATION_API_ID_SPAN: u64 = 8_000_000_000_000_000;

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 14695981039346656037_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

pub(crate) fn notification_api_numeric_id(entry: &NotificationEntry) -> i64 {
    let hash = fnv1a64(entry.id.as_bytes());
    let value = NOTIFICATION_API_ID_BASE + (hash % NOTIFICATION_API_ID_SPAN);
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn notification_api_numeric_id_string(entry: &NotificationEntry) -> String {
    notification_api_numeric_id(entry).to_string()
}

pub(crate) fn notification_entry_matches_cursor_id(
    entry: &NotificationEntry,
    cursor_id: &str,
) -> bool {
    if entry.id == cursor_id {
        return true;
    }
    cursor_id
        .parse::<i64>()
        .ok()
        .is_some_and(|cursor_numeric| notification_api_numeric_id(entry) == cursor_numeric)
}
