use crate::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use crate::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, RemoteStatusRow, actor_url,
    build_local_status_response, build_remote_status_response, can_view_local_status,
    find_account_by_id, find_media_attachments_by_status_id, find_remote_actor_by_actor_uri,
    list_local_status_notifications_for_account, list_remote_status_notifications_for_account,
    load_in_reply_to_account_id, muted_notifications_for_actor, remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

pub(crate) async fn collect_status_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "status") {
        return Ok(());
    }

    for status in
        list_local_status_notifications_for_account(db, &viewer.id, per_type_limit).await?
    {
        let Some(actor) = find_account_by_id(db, &status.account_id).await? else {
            continue;
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
                id: format!("status-local-{}-{}", actor.id, status.id),
                notification_type: "status".to_owned(),
                group_key: format!("status-local-{}-{}", actor.id, status.id),
                created_at: status.created_at,
                account: MastodonAccountResponse::from_account(&actor, config),
                status: Some(status_response),
                report: None,
            },
        );
    }

    for status in
        list_remote_status_notifications_for_account(db, &viewer.id, per_type_limit).await?
    {
        if status.visibility == "direct" {
            continue;
        }
        let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
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
        let status_row = RemoteStatusRow {
            id: status.id.clone(),
            actor_uri: status.actor_uri.clone(),
            object_uri: status.object_uri.clone(),
            url: status.url.clone(),
            in_reply_to_uri: status.in_reply_to_uri.clone(),
            boost_of_uri: status.boost_of_uri.clone(),
            quote_of_uri: status.quote_of_uri.clone(),
            content_html: status.content_html.clone(),
            spoiler_text: status.spoiler_text.clone(),
            visibility: status.visibility.clone(),
            sensitive: status.sensitive,
            language: status.language.clone(),
            quote_state: status.quote_state.clone(),
            published_at: status.published_at.clone(),
        };
        let status_response =
            build_remote_status_response(db, config, Some(viewer), &status_row, &actor).await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("status-remote-{}-{}", remote_id, status.id),
                notification_type: "status".to_owned(),
                group_key: format!("status-remote-{}-{}", remote_id, status.id),
                created_at: status.published_at,
                account: MastodonAccountResponse::from_remote_actor(&actor),
                status: Some(status_response),
                report: None,
            },
        );
    }

    Ok(())
}
