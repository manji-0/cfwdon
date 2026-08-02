use super::{
    AppConfig, NotificationEntry, NotificationsQuery, clear_account_notifications,
    dismiss_account_notification, filter_notification_entries_by_query,
    load_visible_notifications_for_account, notification_group_entries, notifications_fetch_limit,
};
use cfwdon_domain::LocalAccount;
use worker::Result;

use crate::D1Database;
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
    Ok(filter_notification_entries_by_query(entries, query)
        .into_iter()
        .take(limit as usize)
        .collect())
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
            .find(|entry| entry.id == notification_id),
    )
}

pub(crate) async fn dismiss_notification_entry_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    notification_id: &str,
) -> Result<bool> {
    if load_notification_entry_usecase(db, config, viewer, notification_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    dismiss_account_notification(db, viewer.id(), notification_id).await?;
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
    Ok(
        load_visible_notifications_for_account(db, config, viewer, query, per_type_limit)
            .await?
            .len(),
    )
}
