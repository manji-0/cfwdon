use crate::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use crate::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, RemoteStatusRow, StatusRow, actor_url,
    build_local_status_response, build_remote_status_response, can_view_local_status,
    find_account_by_id, find_media_attachments_by_status_id, find_remote_actor_by_actor_uri,
    is_public_activitypub_visibility, list_local_mention_notifications_for_account,
    list_remote_mention_notifications_for_account, load_in_reply_to_account_id,
    muted_notifications_for_actor, remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

pub(crate) async fn collect_mention_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "mention") {
        return Ok(());
    }

    for mention in
        list_local_mention_notifications_for_account(db, viewer, config, per_type_limit).await?
    {
        let Some(actor) = find_account_by_id(db, &mention.account_id).await? else {
            continue;
        };
        let status = StatusRow {
            id: mention.id,
            account_id: mention.account_id.clone(),
            ap_id: mention.ap_id,
            in_reply_to_id: mention.in_reply_to_id,
            boost_of_uri: None,
            quote_of_uri: mention.quote_of_uri,
            content_html: mention.content_html,
            _text_content: mention.text_content,
            spoiler_text: mention.spoiler_text,
            visibility: mention.visibility,
            sensitive: mention.sensitive,
            language: mention.language,
            quote_approval_policy: None,
            quote_state: mention.quote_state.clone(),
            application_id: None,
            created_at: mention.created_at.clone(),
        };
        if !can_view_local_status(db, &status, Some(viewer), &actor).await?
            || muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &actor.username))
                .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), &actor.id, None)
        {
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
                id: format!("mention-local-{}-{}", actor.id, status.id),
                notification_type: "mention".to_owned(),
                group_key: format!("mention-local-{}-{}", actor.id, status.id),
                created_at: status.created_at,
                account: MastodonAccountResponse::from_account(&actor, config),
                status: Some(status_response),
                report: None,
            },
        );
    }

    for mention in
        list_remote_mention_notifications_for_account(db, viewer, config, per_type_limit).await?
    {
        if !is_public_activitypub_visibility(&mention.visibility) {
            continue;
        }
        let Some(actor) = find_remote_actor_by_actor_uri(db, &mention.actor_uri).await? else {
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
        let status = RemoteStatusRow {
            id: mention.id,
            actor_uri: mention.actor_uri.clone(),
            object_uri: mention.object_uri,
            url: mention.url,
            in_reply_to_uri: mention.in_reply_to_uri,
            boost_of_uri: mention.boost_of_uri,
            quote_of_uri: mention.quote_of_uri,
            content_html: mention.content_html,
            spoiler_text: mention.spoiler_text,
            visibility: mention.visibility,
            sensitive: mention.sensitive,
            language: mention.language,
            quote_state: mention.quote_state,
            published_at: mention.published_at.clone(),
        };
        let status_response =
            build_remote_status_response(db, config, Some(viewer), &status, &actor).await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("mention-remote-{}-{}", remote_id, status.id),
                notification_type: "mention".to_owned(),
                group_key: format!("mention-remote-{}-{}", remote_id, status.id),
                created_at: status.published_at,
                account: MastodonAccountResponse::from_remote_actor(&actor),
                status: Some(status_response),
                report: None,
            },
        );
    }

    Ok(())
}
