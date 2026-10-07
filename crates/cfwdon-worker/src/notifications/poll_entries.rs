use super::{
    NotificationCandidate, NotificationsQuery, list_poll_notifications_for_account,
    notification_time_window, notification_type_allowed,
};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;

pub(crate) async fn collect_poll_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "poll") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let polls =
        list_poll_notifications_for_account(db, viewer.id(), per_type_limit, &window).await?;
    candidates.extend(polls.into_iter().map(|poll| {
        NotificationCandidate::authored_local_status_event(
            "poll",
            format!("poll-local-{}", poll.poll_id),
            poll.account_id,
            poll.status_id,
            &poll.expires_at,
        )
    }));
    Ok(())
}
