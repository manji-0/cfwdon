use super::notifications::{
    NotificationEntry, notification_account_matches_filter, notification_type_allowed,
};
use super::{
    AppConfig, MastodonAccountResponse, MentionNotificationRow, NotificationsQuery,
    RemoteMentionNotificationRow, RemoteStatusRecord, RemoteStatusRow, StatusRow, actor_url,
    build_status_notification_entry, can_view_local_status, is_public_activitypub_visibility,
    list_local_mention_notifications_for_account, list_remote_mention_notifications_for_account,
    preload_notification_statuses, remote_account_rest_id, remote_status_from_record,
};
use cfwdon_domain::{LocalAccount, QuoteState, Visibility};
use worker::Result;

use crate::D1Database;
fn local_mention_status_row(mention: MentionNotificationRow) -> Option<StatusRow> {
    let visibility = Visibility::parse(&mention.visibility).ok()?;
    let quote_state = QuoteState::parse(&mention.quote_state).ok()?;
    Some(StatusRow {
        id: mention.id,
        account_id: mention.account_id.clone(),
        ap_id: mention.ap_id,
        in_reply_to_id: mention.in_reply_to_id,
        in_reply_to_account_id: mention.in_reply_to_account_id,
        boost_of_uri: None,
        quote_of_uri: mention.quote_of_uri,
        content_html: mention.content_html,
        text: mention.text_content,
        spoiler_text: mention.spoiler_text,
        visibility,
        sensitive: mention.sensitive != 0,
        language: mention.language,
        quote_approval_policy: None,
        quote_state,
        application_id: None,
        card_json: None,
        created_at: mention.created_at.clone(),
        updated_at: None,
    })
}

fn remote_mention_status_row(mention: RemoteMentionNotificationRow) -> Option<RemoteStatusRow> {
    remote_status_from_record(RemoteStatusRecord {
        id: mention.id,
        actor_uri: mention.actor_uri.clone(),
        object_uri: mention.object_uri,
        url: mention.url,
        in_reply_to_uri: mention.in_reply_to_uri,
        boost_of_uri: mention.boost_of_uri,
        quote_of_uri: mention.quote_of_uri,
        content_html: mention.content_html,
        text_content: mention.text_content,
        spoiler_text: mention.spoiler_text,
        visibility: mention.visibility,
        sensitive: mention.sensitive,
        language: mention.language,
        quote_state: mention.quote_state,
        published_at: mention.published_at.clone(),
        edited_at: mention.edited_at,
        card_json: mention.card_json,
        federated_emojis_json: mention.federated_emojis_json,
        in_reply_to_id: mention.in_reply_to_id,
        favourites_count: None,
        reblogs_count: None,
    })
    .ok()
}

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

    let (local_mentions, remote_mentions) = futures_util::try_join!(
        list_local_mention_notifications_for_account(
            db,
            viewer,
            config,
            per_type_limit,
            query.min_created_at.as_deref(),
        ),
        list_remote_mention_notifications_for_account(
            db,
            viewer,
            config,
            per_type_limit,
            query.min_created_at.as_deref(),
        ),
    )?;
    let local_statuses = local_mentions
        .into_iter()
        .filter_map(local_mention_status_row)
        .collect::<Vec<_>>();
    let remote_statuses = remote_mentions
        .into_iter()
        .filter_map(remote_mention_status_row)
        .collect::<Vec<_>>();
    let preloads = preload_notification_statuses(
        db,
        config,
        viewer,
        &local_statuses,
        &remote_statuses,
        &[],
        &[],
    )
    .await?;
    let preloads_ref = &preloads;

    let mut local_candidates = Vec::new();
    for status in local_statuses {
        let Some(actor) = preloads.local_accounts_by_id.get(&status.account_id) else {
            continue;
        };
        if !can_view_local_status(db, &status, Some(viewer), actor).await?
            || preloads.is_notification_muted(&actor_url(config, actor.username()))
            || !notification_account_matches_filter(query.account_id.as_deref(), actor.id(), None)
        {
            continue;
        }
        local_candidates.push((status, actor.clone()));
    }

    let local_entries = futures_util::future::try_join_all(local_candidates.into_iter().map(
        |(status, actor)| async move {
            let status_response = preloads_ref
                .build_local_status_response(
                    db,
                    config,
                    viewer,
                    &status,
                    &actor,
                    preloads_ref.local_media(&status.id),
                )
                .await?;
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                format!("mention-local-{}-{}", actor.id(), status.id),
                "mention",
                status.created_at,
                MastodonAccountResponse::from_account(&actor, config),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(local_entries);

    let mut remote_candidates = Vec::new();
    for status in remote_statuses {
        if !is_public_activitypub_visibility(status.visibility.as_str())
            && status.visibility.as_str() != "direct"
            && status.visibility.as_str() != "private"
        {
            continue;
        }
        let Some(actor) = preloads.remote_actors_by_uri.get(&status.actor_uri) else {
            continue;
        };
        if preloads.is_notification_muted(&actor.actor_uri) {
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
        remote_candidates.push((status, actor, remote_id));
    }

    let remote_entries = futures_util::future::try_join_all(remote_candidates.into_iter().map(
        |(status, actor, remote_id)| async move {
            let status_response = preloads_ref
                .build_remote_status_response(
                    db,
                    config,
                    viewer,
                    &status,
                    actor,
                    preloads_ref.remote_media(&status.id),
                )
                .await?;
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                format!("mention-remote-{}-{}", remote_id, status.id),
                "mention",
                status.published_at,
                MastodonAccountResponse::from_remote_actor(actor),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(remote_entries);

    Ok(())
}
