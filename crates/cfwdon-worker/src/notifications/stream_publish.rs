use super::types::MastodonNotificationResponse;
use super::{
    AppConfig, MastodonAccountResponse, MastodonStatusResponse, RemoteActorProfile,
    RemoteStatusRow, StatusRecord, StatusRow, build_local_status_response,
    build_remote_status_response, extract_mentions_from_text, find_account_by_id,
    find_account_by_username, find_local_status_by_object_uri, find_media_attachments_by_status_id,
    find_remote_actor_by_actor_uri, is_public_activitypub_visibility, load_in_reply_to_account_id,
    load_remote_status_updated_at, notification_timestamp_sort_token, now_iso_string,
    publish_notification_stream_hub_event_soft, remote_account_rest_id, statuses_from_records,
    strip_html_tags,
};
use crate::timestamp_to_mastodon_iso8601;
use cfwdon_domain::{LocalAccount, QuoteState, Visibility};
use serde::Deserialize;
use worker::d1::D1Type;
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

pub(crate) fn remote_status_update_notification_id(
    remote_id: &str,
    status_id: &str,
    update_token: &str,
) -> String {
    format!("update-remote-{remote_id}-{status_id}-{update_token}")
}

pub(crate) fn remote_quoted_update_notification_id(
    remote_id: &str,
    local_status_id: &str,
    update_token: &str,
) -> String {
    format!("quoted-update-{remote_id}-{local_status_id}-{update_token}")
}

#[derive(Debug, Deserialize)]
struct AccountIdRow {
    account_id: String,
}

async fn load_account_ids(
    db: &D1Database,
    sql: &str,
    bindings: &[D1Type<'_>],
) -> Result<Vec<String>> {
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;
    Ok(result
        .results::<AccountIdRow>()?
        .into_iter()
        .map(|row| row.account_id)
        .collect())
}

async fn list_notify_follower_account_ids_for_remote_actor(
    db: &D1Database,
    actor_uri: &str,
    published_at: &str,
) -> Result<Vec<String>> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Text(published_at)];
    load_account_ids(
        db,
        "SELECT f.follower_account_id AS account_id
         FROM follows f
         WHERE f.target_actor_uri = ?1
           AND f.state = 'accepted'
           AND f.notify = 1
           AND ?2 >= f.updated_at",
        &bindings,
    )
    .await
}

async fn list_reblog_account_ids_for_remote_status(
    db: &D1Database,
    remote_status_id: &str,
) -> Result<Vec<String>> {
    let bindings = [D1Type::Text(remote_status_id)];
    load_account_ids(
        db,
        "SELECT DISTINCT account_id
         FROM reblogs
         WHERE remote_status_id = ?1",
        &bindings,
    )
    .await
}

async fn list_local_quote_statuses_for_remote_object_uri(
    db: &D1Database,
    quote_of_uri: &str,
) -> Result<Vec<StatusRow>> {
    let bindings = [D1Type::Text(quote_of_uri)];
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE quote_of_uri = ?1
               AND quote_state != 'revoked'",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result
        .results::<StatusRecord>()
        .and_then(statuses_from_records)
}

async fn build_remote_status_response_for_recipient_soft(
    db: &D1Database,
    config: &AppConfig,
    recipient_account_id: &str,
    remote_status: &RemoteStatusRow,
    remote_actor: &RemoteActorProfile,
) -> Option<MastodonStatusResponse> {
    let recipient = find_account_by_id(db, recipient_account_id)
        .await
        .ok()
        .flatten()?;
    let actor_row = find_remote_actor_by_actor_uri(db, &remote_actor.actor_uri)
        .await
        .ok()
        .flatten()?;
    build_remote_status_response(db, config, Some(&recipient), remote_status, &actor_row)
        .await
        .ok()
}

async fn build_local_status_response_for_recipient_soft(
    db: &D1Database,
    config: &AppConfig,
    recipient_account_id: &str,
    status: &StatusRow,
    author: &LocalAccount,
) -> Option<MastodonStatusResponse> {
    let recipient = find_account_by_id(db, recipient_account_id)
        .await
        .ok()
        .flatten()?;
    let media = find_media_attachments_by_status_id(db, &status.id)
        .await
        .ok()?;
    let in_reply_to = load_in_reply_to_account_id(db, status).await.ok()?;
    build_local_status_response(
        db,
        config,
        Some(&recipient),
        status,
        author,
        in_reply_to,
        media,
    )
    .await
    .ok()
}

async fn publish_remote_status_stream_notification_soft(
    env: Option<&Env>,
    config: &AppConfig,
    recipient_account_id: &str,
    remote_actor: &RemoteActorProfile,
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
        MastodonAccountResponse::from_remote_actor_profile(remote_actor),
        status,
    );
    publish_notification_response_soft(env, config, recipient_account_id, notification).await;
}

pub(crate) async fn publish_remote_status_create_stream_notifications_soft(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    remote_actor: &RemoteActorProfile,
    remote_status: &RemoteStatusRow,
) {
    if env.is_none() {
        return;
    }

    let remote_id = remote_account_rest_id(&remote_actor.actor_uri);
    let created_at = remote_status.published_at.clone();
    let visibility = remote_status.visibility.as_str();
    let mention_visibility_allowed = is_public_activitypub_visibility(visibility)
        || visibility == "direct"
        || visibility == "private";

    if mention_visibility_allowed {
        let text_content = strip_html_tags(&remote_status.content_html);
        for handle in extract_mentions_from_text(&text_content, config) {
            if let Ok(Some(account)) = find_account_by_username(db, &handle.username).await {
                let recipient_account_id = account.id();
                let id = remote_status_interaction_notification_id(
                    "mention",
                    &remote_id,
                    &remote_status.id,
                );
                let status_response = build_remote_status_response_for_recipient_soft(
                    db,
                    config,
                    recipient_account_id,
                    remote_status,
                    remote_actor,
                )
                .await;
                publish_remote_status_stream_notification_soft(
                    env,
                    config,
                    recipient_account_id,
                    remote_actor,
                    "mention",
                    id.clone(),
                    id,
                    created_at.clone(),
                    status_response,
                )
                .await;
            }
        }
    }

    if let Some(quote_of_uri) = remote_status.quote_of_uri.as_deref()
        && remote_status.effective_quote_state() == QuoteState::Accepted
        && let Ok(Some(target)) = find_local_status_by_object_uri(db, config, quote_of_uri).await
    {
        let recipient_account_id = &target.account_id;
        let id = remote_status_interaction_notification_id("quote", &remote_id, &remote_status.id);
        let status_response = build_remote_status_response_for_recipient_soft(
            db,
            config,
            recipient_account_id,
            remote_status,
            remote_actor,
        )
        .await;
        publish_remote_status_stream_notification_soft(
            env,
            config,
            recipient_account_id,
            remote_actor,
            "quote",
            id.clone(),
            id,
            created_at.clone(),
            status_response,
        )
        .await;
    }

    if remote_status.visibility != Visibility::Direct {
        if let Ok(follower_ids) = list_notify_follower_account_ids_for_remote_actor(
            db,
            &remote_actor.actor_uri,
            &created_at,
        )
        .await
        {
            for recipient_account_id in follower_ids {
                let id = remote_status_interaction_notification_id(
                    "status",
                    &remote_id,
                    &remote_status.id,
                );
                let status_response = build_remote_status_response_for_recipient_soft(
                    db,
                    config,
                    &recipient_account_id,
                    remote_status,
                    remote_actor,
                )
                .await;
                publish_remote_status_stream_notification_soft(
                    env,
                    config,
                    &recipient_account_id,
                    remote_actor,
                    "status",
                    id.clone(),
                    id,
                    created_at.clone(),
                    status_response,
                )
                .await;
            }
        }
    }

    if let Some(in_reply_to_uri) = remote_status.in_reply_to_uri.as_deref()
        && let Ok(Some(parent)) = find_local_status_by_object_uri(db, config, in_reply_to_uri).await
    {
        let recipient_account_id = &parent.account_id;
        let id = remote_status_interaction_notification_id("status", &remote_id, &remote_status.id);
        let status_response = build_remote_status_response_for_recipient_soft(
            db,
            config,
            recipient_account_id,
            remote_status,
            remote_actor,
        )
        .await;
        publish_remote_status_stream_notification_soft(
            env,
            config,
            recipient_account_id,
            remote_actor,
            "status",
            id.clone(),
            id,
            created_at.clone(),
            status_response,
        )
        .await;
    }
}

pub(crate) async fn publish_remote_status_update_stream_notifications_soft(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    remote_actor: &RemoteActorProfile,
    remote_status: &RemoteStatusRow,
) {
    if env.is_none() {
        return;
    }

    let remote_id = remote_account_rest_id(&remote_actor.actor_uri);
    let updated_at = load_remote_status_updated_at(db, &remote_status.id)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| remote_status.published_at.clone());
    let update_token = notification_timestamp_sort_token(&updated_at)
        .unwrap_or_else(|| updated_at.replace([':', ' '], "-"));
    let update_id =
        remote_status_update_notification_id(&remote_id, &remote_status.id, &update_token);

    if let Ok(reblog_accounts) =
        list_reblog_account_ids_for_remote_status(db, &remote_status.id).await
    {
        for recipient_account_id in reblog_accounts {
            let status_response = build_remote_status_response_for_recipient_soft(
                db,
                config,
                &recipient_account_id,
                remote_status,
                remote_actor,
            )
            .await;
            publish_remote_status_stream_notification_soft(
                env,
                config,
                &recipient_account_id,
                remote_actor,
                "update",
                update_id.clone(),
                update_id.clone(),
                updated_at.clone(),
                status_response,
            )
            .await;
        }
    }

    if let Ok(local_quotes) =
        list_local_quote_statuses_for_remote_object_uri(db, &remote_status.object_uri).await
    {
        for local_status in local_quotes {
            let recipient_account_id = &local_status.account_id;
            if let Ok(Some(author)) = find_account_by_id(db, recipient_account_id).await {
                let quoted_update_id = remote_quoted_update_notification_id(
                    &remote_id,
                    &local_status.id,
                    &update_token,
                );
                let status_response = build_local_status_response_for_recipient_soft(
                    db,
                    config,
                    recipient_account_id,
                    &local_status,
                    &author,
                )
                .await;
                publish_remote_status_stream_notification_soft(
                    env,
                    config,
                    recipient_account_id,
                    remote_actor,
                    "quoted_update",
                    quoted_update_id.clone(),
                    quoted_update_id.clone(),
                    updated_at.clone(),
                    status_response,
                )
                .await;
            }
        }
    }
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

    #[test]
    fn remote_status_update_notification_id_matches_collectors() {
        assert_eq!(
            remote_status_update_notification_id("remote-1", "status-9", "20250101120000000"),
            "update-remote-remote-1-status-9-20250101120000000"
        );
    }

    #[test]
    fn remote_quoted_update_notification_id_matches_collectors() {
        assert_eq!(
            remote_quoted_update_notification_id("remote-1", "local-9", "20250101120000000"),
            "quoted-update-remote-1-local-9-20250101120000000"
        );
    }

    #[test]
    fn remote_status_create_notification_ids_match_collectors() {
        assert_eq!(
            remote_status_interaction_notification_id("mention", "remote-1", "status-9"),
            "mention-remote-remote-1-status-9"
        );
        assert_eq!(
            remote_status_interaction_notification_id("quote", "remote-1", "status-9"),
            "quote-remote-remote-1-status-9"
        );
        assert_eq!(
            remote_status_interaction_notification_id("status", "remote-1", "status-9"),
            "status-remote-remote-1-status-9"
        );
    }
}
