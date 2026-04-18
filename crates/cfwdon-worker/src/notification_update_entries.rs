use crate::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use crate::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, build_remote_status_response,
    find_remote_actor_by_actor_uri, list_update_notifications_for_account,
    muted_notifications_for_actor, notification_timestamp_sort_token, remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

pub(crate) async fn collect_update_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "update") {
        return Ok(());
    }

    for update in list_update_notifications_for_account(db, &viewer.id, per_type_limit).await? {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &update.actor_uri).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, &viewer.id, &actor.actor_uri).await? {
            continue;
        }
        let remote_id = remote_account_rest_id(&actor.actor_uri);
        if !notification_account_matches_filter(
            query.account_id.as_deref(),
            &remote_id,
            Some(&actor.actor_uri),
        ) {
            continue;
        }
        let status = update.as_remote_status_row();
        let status_response =
            build_remote_status_response(db, config, Some(viewer), &status, &actor).await?;
        let update_token = notification_timestamp_sort_token(&update.remote_updated_at)
            .unwrap_or_else(|| update.remote_updated_at.replace([':', ' '], "-"));
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("update-remote-{}-{}-{}", remote_id, status.id, update_token),
                notification_type: "update".to_owned(),
                group_key: format!("update-remote-{}-{}-{}", remote_id, status.id, update_token),
                created_at: update.remote_updated_at,
                account: MastodonAccountResponse::from_remote_actor(&actor),
                status: Some(status_response),
                report: None,
            },
        );
    }

    Ok(())
}
