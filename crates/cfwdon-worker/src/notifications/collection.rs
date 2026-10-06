use super::{
    NotificationEntry, NotificationsQuery, collect_admin_report_notifications_entries,
    collect_admin_sign_up_notifications_entries, collect_favourite_notification_entries,
    collect_follow_notification_entries, collect_follow_request_notification_entries,
    collect_mention_notification_entries, collect_poll_notification_entries,
    collect_quote_notification_entries, collect_quoted_update_notification_entries,
    collect_reblog_notification_entries, collect_status_notification_entries,
    collect_update_notification_entries, load_dismissed_notification_ids,
    load_notification_clear_marker, notification_api_numeric_id, notification_timestamp_sort_token,
};
use crate::collections_alpha::collect_collection_notification_entries;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;
macro_rules! collect_notification_batch {
    ($collector:ident, $db:expr, $config:expr, $viewer:expr, $query:expr, $per_type_limit:expr) => {
        async {
            let mut entries = Vec::new();
            $collector(&mut entries, $db, $config, $viewer, $query, $per_type_limit).await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        }
    };
}

pub(crate) async fn collect_visible_notifications(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationEntry>> {
    let (mut entries, dismissed_ids, cleared_at) = futures_util::try_join!(
        collect_notifications(db, config, viewer, query, per_type_limit),
        load_dismissed_notification_ids(db, viewer.id()),
        load_notification_clear_marker(db, viewer.id()),
    )?;
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
    // Newest first by API id, which is time ordered; cursors compare the same ids.
    entries.sort_by_cached_key(|entry| std::cmp::Reverse(notification_api_numeric_id(entry)));
    Ok(entries)
}

pub(crate) async fn collect_notifications(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationEntry>> {
    let batches = futures_util::try_join!(
        collect_notification_batch!(
            collect_admin_report_notifications_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_admin_sign_up_notifications_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_follow_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_follow_request_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_collection_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_favourite_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_mention_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_quote_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_update_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_quoted_update_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_status_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_poll_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_reblog_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
    )?;

    Ok(merge_notification_batches([
        batches.0, batches.1, batches.2, batches.3, batches.4, batches.5, batches.6, batches.7,
        batches.8, batches.9, batches.10, batches.11, batches.12,
    ]))
}

fn merge_notification_batches<const N: usize>(
    batches: [Vec<NotificationEntry>; N],
) -> Vec<NotificationEntry> {
    let mut entries = Vec::new();
    for batch in batches {
        entries.extend(batch);
    }
    entries
}
