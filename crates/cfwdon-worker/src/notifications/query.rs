use super::{NotificationEntry, NotificationsQuery, notification_sort_key};

fn normalized_notification_cursor(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn notification_cursor_key(entry: &NotificationEntry) -> (String, String) {
    (notification_sort_key(&entry.created_at), entry.id.clone())
}

fn resolve_notification_cursor_key(
    entries: &[NotificationEntry],
    cursor_id: Option<&str>,
) -> Option<(String, String)> {
    let cursor_id = normalized_notification_cursor(cursor_id)?;
    entries
        .iter()
        .find(|entry| entry.id == cursor_id)
        .map(notification_cursor_key)
}

pub(crate) fn filter_notification_entries_by_query(
    entries: Vec<NotificationEntry>,
    query: &NotificationsQuery,
) -> Vec<NotificationEntry> {
    let max_cursor = resolve_notification_cursor_key(&entries, query.max_id.as_deref());
    let min_cursor = resolve_notification_cursor_key(
        &entries,
        query.min_id.as_deref().or(query.since_id.as_deref()),
    );

    entries
        .into_iter()
        .filter(|entry| {
            let cursor_key = notification_cursor_key(entry);
            max_cursor.as_ref().is_none_or(|value| cursor_key < *value)
                && min_cursor.as_ref().is_none_or(|value| cursor_key > *value)
        })
        .collect()
}

pub(crate) fn notifications_fetch_limit(query: &NotificationsQuery, limit: u32) -> u32 {
    if query.max_id.is_some() || query.since_id.is_some() || query.min_id.is_some() {
        1000
    } else {
        limit.saturating_mul(4)
    }
}

fn notification_group_key(entry: &NotificationEntry) -> &str {
    entry
        .value
        .get("group_key")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(entry.id.as_str())
}

pub(crate) fn notification_group_entries<'a>(
    entries: &'a [NotificationEntry],
    group_key: &str,
) -> Vec<&'a NotificationEntry> {
    entries
        .iter()
        .filter(|entry| notification_group_key(entry) == group_key)
        .collect()
}
