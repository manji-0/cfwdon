use super::{
    NotificationCandidate, NotificationsQuery, list_update_notifications_for_account,
    notification_time_window, notification_timestamp_sort_token, notification_type_allowed,
};
use crate::identity::remote_account_rest_id;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;

pub(crate) async fn collect_update_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "update") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let updates =
        list_update_notifications_for_account(db, viewer.id(), per_type_limit, &window).await?;
    candidates.extend(updates.into_iter().filter_map(|update| {
        let status = update.as_remote_status_row().ok()?;
        let update_token = notification_timestamp_sort_token(&update.remote_updated_at)
            .unwrap_or_else(|| update.remote_updated_at.replace([':', ' '], "-"));
        let id = format!(
            "update-remote-{}-{}-{}",
            remote_account_rest_id(&status.actor_uri),
            status.id,
            update_token
        );
        Some(NotificationCandidate::authored_remote_status_event(
            "update",
            id,
            &update.remote_updated_at,
            status,
        ))
    }));
    Ok(())
}
