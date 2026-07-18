use super::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, RemoteStatusNotificationRow,
    RemoteStatusRecord, RemoteStatusRow, actor_url, build_local_status_response,
    build_remote_status_response, can_view_local_status, find_accounts_by_ids,
    find_media_attachments_by_status_ids, find_remote_actors_by_actor_uris,
    list_local_status_notifications_for_account, list_remote_status_notifications_for_account,
    load_in_reply_to_account_ids, muted_notifications_for_actor, remote_account_rest_id,
    remote_status_from_record,
};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

fn remote_status_notification_row(status: &RemoteStatusNotificationRow) -> Option<RemoteStatusRow> {
    remote_status_from_record(RemoteStatusRecord {
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
    })
    .ok()
}

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

    let local_statuses =
        list_local_status_notifications_for_account(db, viewer.id(), per_type_limit).await?;
    let local_account_ids = local_statuses
        .iter()
        .map(|status| status.account_id.clone())
        .collect::<Vec<_>>();
    let local_status_ids = local_statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();
    let (local_accounts, mut media_by_status_id, in_reply_to_account_ids) = futures_util::try_join!(
        find_accounts_by_ids(db, &local_account_ids),
        find_media_attachments_by_status_ids(db, &local_status_ids),
        load_in_reply_to_account_ids(db, &local_statuses),
    )?;

    for status in local_statuses {
        let Some(actor) = local_accounts.get(&status.account_id) else {
            continue;
        };
        if !can_view_local_status(db, &status, Some(viewer), actor).await?
            || muted_notifications_for_actor(db, viewer.id(), &actor_url(config, actor.username()))
                .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), actor.id(), None)
        {
            continue;
        }
        let media = media_by_status_id.remove(&status.id).unwrap_or_default();
        let status_response = build_local_status_response(
            db,
            config,
            Some(viewer),
            &status,
            actor,
            in_reply_to_account_ids.get(&status.id).cloned(),
            media,
        )
        .await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("status-local-{}-{}", actor.id(), status.id),
                notification_type: "status".to_owned(),
                group_key: format!("status-local-{}-{}", actor.id(), status.id),
                created_at: status.created_at,
                account: MastodonAccountResponse::from_account(actor, config),
                status: Some(status_response),
                report: None,
            },
        );
    }

    let remote_statuses =
        list_remote_status_notifications_for_account(db, viewer.id(), per_type_limit).await?;
    let remote_actor_uris = remote_statuses
        .iter()
        .map(|status| status.actor_uri.clone())
        .collect::<Vec<_>>();
    let remote_actors = find_remote_actors_by_actor_uris(db, &remote_actor_uris).await?;

    for status in remote_statuses {
        if status.visibility == "direct" {
            continue;
        }
        let Some(actor) = remote_actors.get(&status.actor_uri) else {
            continue;
        };
        if muted_notifications_for_actor(db, viewer.id(), &actor.actor_uri).await? {
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
        let Some(status_row) = remote_status_notification_row(&status) else {
            continue;
        };
        let status_response =
            build_remote_status_response(db, config, Some(viewer), &status_row, actor).await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("status-remote-{}-{}", remote_id, status.id),
                notification_type: "status".to_owned(),
                group_key: format!("status-remote-{}-{}", remote_id, status.id),
                created_at: status.published_at,
                account: MastodonAccountResponse::from_remote_actor(actor),
                status: Some(status_response),
                report: None,
            },
        );
    }

    Ok(())
}
