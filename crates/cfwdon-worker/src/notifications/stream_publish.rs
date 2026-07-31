use super::{
    AppConfig, MastodonAccountResponse, MastodonStatusResponse, StatusRow,
    build_local_status_response, find_account_by_id, find_media_attachments_by_status_id,
    load_in_reply_to_account_id, now_iso_string, publish_notification_stream_hub_event_soft,
};
use super::types::MastodonNotificationResponse;
use crate::timestamp_to_mastodon_iso8601;
use cfwdon_domain::LocalAccount;
use worker::{console_error, D1Database, Env, Result};

pub(crate) fn local_notification_response(
    id: String,
    notification_type: &str,
    group_key: String,
    created_at: String,
    account: MastodonAccountResponse,
    status: Option<MastodonStatusResponse>,
) -> MastodonNotificationResponse {
    MastodonNotificationResponse {
        id,
        notification_type: notification_type.to_owned(),
        group_key,
        created_at,
        account,
        status,
        report: None,
    }
}

pub(crate) fn local_status_interaction_notification_id(
    notification_type: &str,
    actor_id: &str,
    status_id: &str,
) -> String {
    format!("{notification_type}-local-{actor_id}-{status_id}")
}

pub(crate) async fn publish_notification_response_soft(
    env: Option<&Env>,
    config: &AppConfig,
    recipient_account_id: &str,
    notification: MastodonNotificationResponse,
) {
    let Some(env) = env else {
        return;
    };

    let mut notification = notification;
    notification.created_at = timestamp_to_mastodon_iso8601(&notification.created_at);
    let notification_id = notification.id.clone();

    let payload = match serde_json::to_string(&notification) {
        Ok(payload) => payload,
        Err(error) => {
            console_error!("failed to serialize notification for stream hub: {error}");
            return;
        }
    };

    publish_notification_stream_hub_event_soft(
        env,
        &config.stream_hub_binding,
        recipient_account_id,
        &payload,
        Some(&notification_id),
    )
    .await;
}

pub(crate) async fn publish_local_actor_notification_soft(
    env: Option<&Env>,
    _db: &D1Database,
    config: &AppConfig,
    recipient_account_id: &str,
    actor: &LocalAccount,
    notification_type: &str,
    id: String,
    group_key: String,
    created_at: String,
    status: Option<MastodonStatusResponse>,
) {
    let notification = local_notification_response(
        id,
        notification_type,
        group_key,
        created_at,
        MastodonAccountResponse::from_account(actor, config),
        status,
    );
    publish_notification_response_soft(env, config, recipient_account_id, notification).await;
}

pub(crate) async fn publish_local_status_interaction_notification_soft(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    recipient_account_id: &str,
    actor: &LocalAccount,
    notification_type: &str,
    status: &StatusRow,
) -> Result<()> {
    if recipient_account_id == actor.id() {
        return Ok(());
    }

    let id = local_status_interaction_notification_id(notification_type, actor.id(), &status.id);
    let group_key = id.clone();

    let Some(recipient) = find_account_by_id(db, recipient_account_id).await? else {
        return Ok(());
    };

    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    let status_response = build_local_status_response(
        db,
        config,
        Some(&recipient),
        status,
        &recipient,
        load_in_reply_to_account_id(db, status).await?,
        media,
    )
    .await?;

    let created_at = now_iso_string()?;

    publish_local_actor_notification_soft(
        env,
        db,
        config,
        recipient_account_id,
        actor,
        notification_type,
        id,
        group_key,
        created_at,
        Some(status_response),
    )
    .await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_status_interaction_notification_id_matches_collectors() {
        assert_eq!(
            local_status_interaction_notification_id("favourite", "actor-1", "status-9"),
            "favourite-local-actor-1-status-9"
        );
        assert_eq!(
            local_status_interaction_notification_id("reblog", "actor-1", "status-9"),
            "reblog-local-actor-1-status-9"
        );
    }
}
