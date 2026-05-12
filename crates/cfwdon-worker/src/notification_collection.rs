use crate::{
    AppConfig, NotificationEntry, NotificationsQuery, collect_admin_report_notifications_entries,
    collect_admin_sign_up_notifications_entries, collect_collection_notification_entries,
    collect_favourite_notification_entries, collect_follow_notification_entries,
    collect_follow_request_notification_entries, collect_mention_notification_entries,
    collect_poll_notification_entries, collect_quote_notification_entries,
    collect_quoted_update_notification_entries, collect_reblog_notification_entries,
    collect_status_notification_entries, collect_update_notification_entries,
    load_dismissed_notification_ids, load_notification_clear_marker, notification_sort_key,
    notification_timestamp_sort_token,
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
    let (
        mut admin_reports,
        admin_signups,
        follows,
        follow_requests,
        collections,
        favourites,
        mentions,
        quotes,
        updates,
        quoted_updates,
        statuses,
        polls,
        reblogs,
    ) = futures_util::try_join!(
        async {
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
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_admin_sign_up_notifications_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_follow_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_follow_request_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_collection_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_favourite_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_mention_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_quote_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_update_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_quoted_update_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_status_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_poll_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
        async {
            let mut entries = Vec::new();
            collect_reblog_notification_entries(
                &mut entries,
                db,
                config,
                viewer,
                query,
                per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationEntry>, worker::Error>(entries)
        },
    )?;

    let mut entries = Vec::new();
    entries.append(&mut admin_reports);
    entries.extend(admin_signups);
    entries.extend(follows);
    entries.extend(follow_requests);
    entries.extend(collections);
    entries.extend(favourites);
    entries.extend(mentions);
    entries.extend(quotes);
    entries.extend(updates);
    entries.extend(quoted_updates);
    entries.extend(statuses);
    entries.extend(polls);
    entries.extend(reblogs);
    Ok(entries)
}
