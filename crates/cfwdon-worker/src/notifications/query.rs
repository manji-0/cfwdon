use super::{
    NotificationIdentity, NotificationsQuery, is_legacy_notification_api_id,
    notification_api_numeric_id, notification_entry_matches_cursor_id,
    notification_query_has_untimed_cursor,
};

fn normalized_notification_cursor(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// API ids are time ordered, so a numeric cursor bounds the page even when its
/// notification has dropped out of the loaded window. Internal keys from
/// before API ids existed still resolve through the window.
pub(crate) fn resolve_notification_cursor_key<T: NotificationIdentity>(
    entries: &[T],
    cursor_id: Option<&str>,
) -> Option<i64> {
    let cursor_id = normalized_notification_cursor(cursor_id)?;
    if let Ok(api_id) = cursor_id.parse::<i64>()
        && !is_legacy_notification_api_id(api_id)
    {
        return Some(api_id);
    }
    entries
        .iter()
        .find(|entry| notification_entry_matches_cursor_id(entry, cursor_id))
        .map(notification_api_numeric_id)
}

pub(crate) fn filter_notification_entries_by_query<T: NotificationIdentity>(
    entries: Vec<T>,
    query: &NotificationsQuery,
) -> Vec<T> {
    let max_cursor = resolve_notification_cursor_key(&entries, query.max_id.as_deref());
    let min_cursor = resolve_notification_cursor_key(
        &entries,
        query.min_id.as_deref().or(query.since_id.as_deref()),
    );

    entries
        .into_iter()
        .filter(|entry| {
            let cursor_key = notification_api_numeric_id(entry);
            max_cursor.is_none_or(|value| cursor_key < value)
                && min_cursor.is_none_or(|value| cursor_key > value)
        })
        .collect()
}

/// Rows each source reads for one page. Timed cursors bound every source in
/// SQL, so a newest-first page only needs a margin over `limit` for rows that
/// mutes, filters and dismissals drop. `min_id` wants the oldest rows after
/// its cursor while sources sort newest first, and untimed cursors are only
/// resolved in Rust, so both still read a wide window.
pub(crate) fn notifications_fetch_limit(query: &NotificationsQuery, limit: u32) -> u32 {
    let forward = query
        .min_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if forward || notification_query_has_untimed_cursor(query) {
        1000
    } else {
        limit.saturating_mul(4)
    }
}
