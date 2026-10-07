use super::{
    NotificationCandidate, NotificationEntry, NotificationIdentity, NotificationsQuery,
    clear_account_notifications, collect_notification_candidates,
    default_grouped_notification_types, dismiss_account_notification,
    filter_notification_entries_by_query, hydrate_notification_candidates,
    hydrate_notification_page, notification_api_numeric_id, notification_entry_matches_cursor_id,
    notification_group_candidates, notifications_fetch_limit, resolve_notification_cursor_key,
};
use crate::markers::load_notifications_last_read_id;
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
    let candidates = collect_notification_candidates(
        db,
        config,
        viewer,
        query,
        notifications_fetch_limit(query, limit),
    )
    .await?;
    let candidates = filter_notification_entries_by_query(candidates, query);
    // `min_id` asks for the page just after the cursor, i.e. the oldest
    // matches; `since_id` keeps the newest.
    let forward = query
        .min_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    hydrate_notification_page(db, config, viewer, candidates, limit as usize, forward).await
}

async fn load_notification_group_candidates(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
    group_key: &str,
) -> Result<Vec<NotificationCandidate>> {
    let candidates =
        collect_notification_candidates(db, config, viewer, query, per_type_limit).await?;
    Ok(notification_group_candidates(
        candidates,
        group_key,
        &default_grouped_notification_types(),
    ))
}

pub(crate) async fn list_notification_group_entries_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
    group_key: &str,
) -> Result<Vec<NotificationEntry>> {
    let candidates =
        load_notification_group_candidates(db, config, viewer, query, per_type_limit, group_key)
            .await?;
    hydrate_notification_candidates(db, config, viewer, candidates).await
}

async fn find_notification_candidate(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    notification_id: &str,
) -> Result<Option<NotificationCandidate>> {
    // A snowflake id names its creation second; bounding both ends by it keeps
    // every source to that second. Untimed ids still scan the newest rows.
    let query = NotificationsQuery {
        limit: Some(200),
        max_id: Some(notification_id.to_owned()),
        since_id: Some(notification_id.to_owned()),
        ..NotificationsQuery::default()
    };
    Ok(
        collect_notification_candidates(db, config, viewer, &query, 200)
            .await?
            .into_iter()
            .find(|candidate| notification_entry_matches_cursor_id(candidate, notification_id)),
    )
}

pub(crate) async fn load_notification_entry_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    notification_id: &str,
) -> Result<Option<NotificationEntry>> {
    let Some(candidate) = find_notification_candidate(db, config, viewer, notification_id).await?
    else {
        return Ok(None);
    };
    Ok(
        hydrate_notification_candidates(db, config, viewer, vec![candidate])
            .await?
            .into_iter()
            .next(),
    )
}

pub(crate) async fn dismiss_notification_entry_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    notification_id: &str,
) -> Result<bool> {
    let Some(candidate) = find_notification_candidate(db, config, viewer, notification_id).await?
    else {
        return Ok(false);
    };
    // Dismissals are stored under the internal key the collectors filter on.
    dismiss_account_notification(db, viewer.id(), &candidate.id).await?;
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
    let candidates =
        load_notification_group_candidates(db, config, viewer, query, per_type_limit, group_key)
            .await?;
    if candidates.is_empty() {
        return Ok(false);
    }
    for candidate in candidates {
        dismiss_account_notification(db, viewer.id(), &candidate.id).await?;
    }
    Ok(true)
}

pub(crate) async fn clear_notifications_usecase(
    db: &D1Database,
    viewer: &LocalAccount,
) -> Result<()> {
    clear_account_notifications(db, viewer.id()).await
}

/// Counted from candidates, so nothing is rendered.
pub(crate) async fn unread_notifications_count_usecase(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<usize> {
    let (candidates, last_read_id) = futures_util::try_join!(
        collect_notification_candidates(db, config, viewer, query, per_type_limit),
        load_notifications_last_read_id(db, viewer.id()),
    )?;
    Ok(count_unread_notification_entries(
        &candidates,
        last_read_id.as_deref(),
    ))
}

/// Mastodon counts notifications newer than the notifications marker. A marker
/// that is not in the loaded window is older than all of it, so the whole
/// window is unread.
pub(crate) fn count_unread_notification_entries<T: NotificationIdentity>(
    entries: &[T],
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
