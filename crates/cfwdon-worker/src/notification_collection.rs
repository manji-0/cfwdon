use crate::{
    AppConfig, NotificationEntry, NotificationsQuery, collect_admin_report_notifications_entries,
    collect_admin_sign_up_notifications_entries, collect_favourite_notification_entries,
    collect_follow_notification_entries, collect_mention_notification_entries,
    collect_poll_notification_entries, collect_reblog_notification_entries,
    collect_status_notification_entries, load_dismissed_notification_ids,
    load_notification_clear_marker, notification_sort_key, notification_timestamp_sort_token,
};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

pub(crate) async fn collect_visible_notifications(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationEntry>> {
    let mut entries = collect_notifications(db, config, viewer, query, per_type_limit).await?;
    let dismissed_ids = load_dismissed_notification_ids(db, &viewer.id).await?;
    let cleared_at = load_notification_clear_marker(db, &viewer.id).await?;
    let cleared_at_token = cleared_at
        .as_deref()
        .and_then(notification_timestamp_sort_token);

    entries.retain(|entry| {
        if dismissed_ids.contains(&entry.id) {
            return false;
        }
        match (
            cleared_at_token.as_deref(),
            notification_timestamp_sort_token(&entry.created_at),
        ) {
            (Some(cleared_at), Some(created_at)) => created_at.as_str() > cleared_at,
            _ => true,
        }
    });
    entries.sort_by(|left, right| {
        notification_sort_key(&right.created_at)
            .cmp(&notification_sort_key(&left.created_at))
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(entries)
}

pub(crate) async fn collect_notifications(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationEntry>> {
    let mut entries = Vec::new();

    collect_admin_report_notifications_entries(
        &mut entries,
        db,
        config,
        viewer,
        query,
        per_type_limit,
    )
    .await?;
    collect_admin_sign_up_notifications_entries(
        &mut entries,
        db,
        config,
        viewer,
        query,
        per_type_limit,
    )
    .await?;
    collect_follow_notification_entries(&mut entries, db, config, viewer, query, per_type_limit)
        .await?;
    collect_favourite_notification_entries(&mut entries, db, config, viewer, query, per_type_limit)
        .await?;
    collect_mention_notification_entries(&mut entries, db, config, viewer, query, per_type_limit)
        .await?;
    collect_status_notification_entries(&mut entries, db, config, viewer, query, per_type_limit)
        .await?;
    collect_poll_notification_entries(&mut entries, db, config, viewer, query, per_type_limit)
        .await?;
    collect_reblog_notification_entries(&mut entries, db, config, viewer, query, per_type_limit)
        .await?;

    Ok(entries)
}
