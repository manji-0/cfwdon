use crate::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use crate::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, actor_url, build_local_status_response,
    find_account_by_id, find_media_attachments_by_status_id, find_status_by_id,
    list_poll_notifications_for_account, load_in_reply_to_account_id,
    muted_notifications_for_actor,
};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

pub(crate) async fn collect_poll_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "poll") {
        return Ok(());
    }

    for poll in list_poll_notifications_for_account(db, &viewer.id, per_type_limit).await? {
        let Some(actor) = find_account_by_id(db, &poll.account_id).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &actor.username))
            .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), &actor.id, None)
        {
            continue;
        }
        let Some(status) = find_status_by_id(db, &poll.status_id).await? else {
            continue;
        };
        if !crate::can_view_local_status(db, &status, Some(viewer), &actor).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        let status_response = build_local_status_response(
            db,
            config,
            Some(viewer),
            &status,
            &actor,
            load_in_reply_to_account_id(db, &status).await?,
            media,
        )
        .await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("poll-local-{}", poll.poll_id),
                notification_type: "poll".to_owned(),
                group_key: format!("poll-local-{}", poll.poll_id),
                created_at: poll.expires_at,
                account: MastodonAccountResponse::from_account(&actor, config),
                status: Some(status_response),
                report: None,
            },
        );
    }

    Ok(())
}
