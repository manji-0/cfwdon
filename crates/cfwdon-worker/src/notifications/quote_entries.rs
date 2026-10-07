use super::{
    NotificationActorRef, NotificationCandidate, NotificationsQuery,
    list_local_quote_notifications_for_account, list_quoted_update_notifications_for_account,
    list_remote_quote_notifications_for_account, notification_time_window,
    notification_timestamp_sort_token, notification_type_allowed,
};
use crate::identity::remote_account_rest_id;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;

pub(crate) async fn collect_quote_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "quote") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let (local_quotes, remote_quotes) = futures_util::try_join!(
        list_local_quote_notifications_for_account(db, viewer.id(), per_type_limit, &window),
        list_remote_quote_notifications_for_account(db, viewer.id(), per_type_limit, &window),
    )?;
    candidates.extend(
        local_quotes
            .into_iter()
            .map(|status| NotificationCandidate::authored_local_status("quote", status)),
    );
    candidates.extend(
        remote_quotes
            .into_iter()
            .map(|status| NotificationCandidate::authored_remote_status("quote", status)),
    );
    Ok(())
}

pub(crate) async fn collect_quoted_update_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "quoted_update") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let updates =
        list_quoted_update_notifications_for_account(db, viewer.id(), per_type_limit, &window)
            .await?;
    candidates.extend(updates.into_iter().map(|update| {
        let update_token = notification_timestamp_sort_token(&update.remote_updated_at)
            .unwrap_or_else(|| update.remote_updated_at.replace([':', ' '], "-"));
        let id = format!(
            "quoted-update-{}-{}-{}",
            remote_account_rest_id(&update.remote_actor_uri),
            update.id,
            update_token
        );
        NotificationCandidate::viewer_status_event(
            "quoted_update",
            id,
            NotificationActorRef::Remote(update.remote_actor_uri),
            update.id,
            &update.remote_updated_at,
        )
    }));
    Ok(())
}
