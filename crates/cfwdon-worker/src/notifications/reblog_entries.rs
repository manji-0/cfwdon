use super::{
    NotificationActorRef, NotificationCandidate, NotificationsQuery,
    list_reblog_notifications_for_account, list_remote_reblog_notifications_for_account,
    notification_time_window, notification_type_allowed,
};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;

pub(crate) async fn collect_reblog_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "reblog") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let (local, remote) = futures_util::try_join!(
        list_reblog_notifications_for_account(db, viewer.id(), per_type_limit, &window),
        list_remote_reblog_notifications_for_account(db, viewer.id(), per_type_limit, &window),
    )?;
    candidates.extend(local.into_iter().map(|row| {
        NotificationCandidate::viewer_status_interaction(
            "reblog",
            NotificationActorRef::Local(row.account_id),
            row.status_id,
            &row.created_at,
        )
    }));
    candidates.extend(remote.into_iter().map(|row| {
        NotificationCandidate::viewer_status_interaction(
            "reblog",
            NotificationActorRef::Remote(row.remote_actor_uri),
            row.status_id,
            &row.created_at,
        )
    }));
    Ok(())
}
