use super::types::MastodonNotificationResponse;
use super::{
    AppConfig, MastodonAccountResponse, MastodonStatusResponse, RemoteActorProfile, StatusRow,
    build_local_status_response, find_account_by_id, find_media_attachments_by_status_id,
    load_in_reply_to_account_id, now_iso_string, publish_notification_stream_hub_event_soft,
    remote_account_rest_id,
};
use crate::timestamp_to_mastodon_iso8601;
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Env, Result, console_error};

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

pub(crate) fn remote_status_interaction_notification_id(
    notification_type: &str,
    remote_id: &str,
    status_id: &str,
) -> String {
    format!("{notification_type}-remote-{remote_id}-{status_id}")
}

pub(crate) fn remote_actor_notification_id(notification_type: &str, remote_id: &str) -> String {
    let prefix = if notification_type == "follow_request" {
        "follow-request"
    } else {
        notification_type
    };
    format!("{prefix}-remote-{remote_id}")
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

pub(crate) async fn publish_remote_actor_notification_soft(
    env: Option<&Env>,
    config: &AppConfig,
    recipient_account_id: &str,
    remote_actor: &RemoteActorProfile,
    notification_type: &str,
    created_at: String,
) {
    let remote_id = remote_account_rest_id(&remote_actor.actor_uri);
    let id = remote_actor_notification_id(notification_type, &remote_id);
    let notification = local_notification_response(
        id.clone(),
        notification_type,
        id,
        created_at,
        MastodonAccountResponse::from_remote_actor_profile(remote_actor),
        None,
    );
    publish_notification_response_soft(env, config, recipient_account_id, notification).await;
}

pub(crate) async fn publish_remote_status_interaction_notification_soft(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    recipient_account_id: &str,
    remote_actor: &RemoteActorProfile,
    notification_type: &str,
    status: &StatusRow,
) -> Result<()> {
    let remote_id = remote_account_rest_id(&remote_actor.actor_uri);
    let id = remote_status_interaction_notification_id(notification_type, &remote_id, &status.id);
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

    let notification = local_notification_response(
        id,
        notification_type,
        group_key,
        created_at,
        MastodonAccountResponse::from_remote_actor_profile(remote_actor),
        Some(status_response),
    );
    publish_notification_response_soft(env, config, recipient_account_id, notification).await;

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

    #[test]
    fn remote_status_interaction_notification_id_matches_collectors() {
        assert_eq!(
            remote_status_interaction_notification_id("favourite", "remote-1", "status-9"),
            "favourite-remote-remote-1-status-9"
        );
        assert_eq!(
            remote_status_interaction_notification_id("reblog", "remote-1", "status-9"),
            "reblog-remote-remote-1-status-9"
        );
    }

    #[test]
    fn remote_actor_notification_id_matches_collectors() {
        assert_eq!(
            remote_actor_notification_id("follow", "remote-1"),
            "follow-remote-remote-1"
        );
        assert_eq!(
            remote_actor_notification_id("follow_request", "remote-1"),
            "follow-request-remote-remote-1"
        );
    }
}
