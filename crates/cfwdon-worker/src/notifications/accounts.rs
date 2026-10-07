use super::{
    NotificationActorRef, NotificationCandidate, NotificationsQuery,
    list_favourite_notifications_for_account, list_local_follow_notifications_for_account,
    list_local_follow_request_notifications_for_account,
    list_remote_favourite_notifications_for_account, list_remote_follow_notifications_for_account,
    list_remote_follow_request_notifications_for_account, notification_time_window,
    notification_type_allowed,
};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;

pub(crate) async fn collect_follow_request_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "follow_request") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let (local, remote) = futures_util::try_join!(
        list_local_follow_request_notifications_for_account(
            db,
            viewer.id(),
            per_type_limit,
            &window
        ),
        list_remote_follow_request_notifications_for_account(
            db,
            viewer.id(),
            per_type_limit,
            &window
        ),
    )?;
    candidates.extend(local.into_iter().map(|row| {
        NotificationCandidate::account(
            "follow_request",
            "follow-request",
            NotificationActorRef::Local(row.follower_account_id),
            &row.created_at,
        )
    }));
    candidates.extend(remote.into_iter().map(|row| {
        NotificationCandidate::account(
            "follow_request",
            "follow-request",
            NotificationActorRef::Remote(row.actor_uri),
            &row.created_at,
        )
    }));
    Ok(())
}

pub(crate) async fn collect_follow_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "follow") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let (local, remote) = futures_util::try_join!(
        list_local_follow_notifications_for_account(db, viewer.id(), per_type_limit, &window),
        list_remote_follow_notifications_for_account(db, viewer.id(), per_type_limit, &window),
    )?;
    candidates.extend(local.into_iter().map(|row| {
        NotificationCandidate::account(
            "follow",
            "follow",
            NotificationActorRef::Local(row.follower_account_id),
            &row.created_at,
        )
    }));
    candidates.extend(remote.into_iter().map(|row| {
        NotificationCandidate::account(
            "follow",
            "follow",
            NotificationActorRef::Remote(row.actor_uri),
            &row.created_at,
        )
    }));
    Ok(())
}

pub(crate) async fn collect_favourite_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "favourite") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let (local, remote) = futures_util::try_join!(
        list_favourite_notifications_for_account(db, viewer.id(), per_type_limit, &window),
        list_remote_favourite_notifications_for_account(db, viewer.id(), per_type_limit, &window),
    )?;
    candidates.extend(local.into_iter().map(|row| {
        NotificationCandidate::viewer_status_interaction(
            "favourite",
            NotificationActorRef::Local(row.account_id),
            row.status_id,
            &row.created_at,
        )
    }));
    candidates.extend(remote.into_iter().map(|row| {
        NotificationCandidate::viewer_status_interaction(
            "favourite",
            NotificationActorRef::Remote(row.remote_actor_uri),
            row.status_id,
            &row.created_at,
        )
    }));
    Ok(())
}
