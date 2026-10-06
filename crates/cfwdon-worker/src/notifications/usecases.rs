use super::{
    NotificationEntry, NotificationsQuery, clear_account_notifications,
    dismiss_account_notification, filter_notification_entries_by_query,
    load_visible_notifications_for_account, notification_api_numeric_id,
    notification_entry_matches_cursor_id, notification_group_entries, notifications_fetch_limit,
    resolve_notification_cursor_key,
};
use crate::markers::load_notifications_last_read_id;
use crate::timelines::keep_timeline_page;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;
pub(crate) async fn list_notifications_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    limit: u32,
) -> Result<Vec<NotificationEntry>> {
    let entries = load_visible_notifications_for_account(
        db,
        config,
        viewer,
        query,
        notifications_fetch_limit(query, limit),
    )
    .await?;
    let mut entries = filter_notification_entries_by_query(entries, query);
    // `min_id` asks for the page just after the cursor, i.e. the oldest
    // matches; `since_id` keeps the newest.
    let forward = query
        .min_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    keep_timeline_page(&mut entries, limit as usize, forward);
    Ok(entries)
}

pub(crate) async fn list_notification_group_entries_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
    group_key: &str,
) -> Result<Vec<NotificationEntry>> {
    let entries =
        load_visible_notifications_for_account(db, config, viewer, query, per_type_limit).await?;
    Ok(notification_group_entries(&entries, group_key)
        .into_iter()
        .cloned()
        .collect())
}

pub(crate) async fn load_notification_entry_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    notification_id: &str,
) -> Result<Option<NotificationEntry>> {
    let query = NotificationsQuery {
        limit: Some(200),
        ..NotificationsQuery::default()
    };
    Ok(
        load_visible_notifications_for_account(db, config, viewer, &query, 200)
            .await?
            .into_iter()
            .find(|entry| notification_entry_matches_cursor_id(entry, notification_id)),
    )
}

pub(crate) async fn dismiss_notification_entry_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    notification_id: &str,
) -> Result<bool> {
    let Some(entry) = load_notification_entry_usecase(db, config, viewer, notification_id).await?
    else {
        return Ok(false);
    };
    // Dismissals are stored under the internal key the collectors filter on.
    dismiss_account_notification(db, viewer.id(), &entry.id).await?;
    Ok(true)
}

pub(crate) async fn dismiss_notification_group_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
    group_key: &str,
) -> Result<bool> {
    let entries = list_notification_group_entries_usecase(
        db,
        config,
        viewer,
        query,
        per_type_limit,
        group_key,
    )
    .await?;
    if entries.is_empty() {
        return Ok(false);
    }
    for entry in entries {
        dismiss_account_notification(db, viewer.id(), &entry.id).await?;
    }
    Ok(true)
}

pub(crate) async fn clear_notifications_usecase(
    db: &D1Database,
    viewer: &LocalAccount,
) -> Result<()> {
    clear_account_notifications(db, viewer.id()).await
}

pub(crate) async fn unread_notifications_count_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<usize> {
    let (entries, last_read_id) = futures_util::try_join!(
        load_visible_notifications_for_account(db, config, viewer, query, per_type_limit),
        load_notifications_last_read_id(db, viewer.id()),
    )?;
    Ok(count_unread_notification_entries(
        &entries,
        last_read_id.as_deref(),
    ))
}

/// Mastodon counts notifications newer than the notifications marker. A marker
/// that is not in the loaded window is older than all of it, so the whole
/// window is unread.
pub(crate) fn count_unread_notification_entries(
    entries: &[NotificationEntry],
    last_read_id: Option<&str>,
) -> usize {
    let Some(last_read_id) = last_read_id else {
        return entries.len();
    };
    let Some(marker) = resolve_notification_cursor_key(entries, Some(last_read_id)) else {
        return entries.len();
    };
    entries
        .iter()
        .filter(|entry| notification_api_numeric_id(entry) > marker)
        .count()
}
