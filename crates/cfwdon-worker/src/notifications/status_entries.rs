use super::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use super::{
    AppConfig, LocalAccount, MastodonAccountResponse, MastodonStatusResponse,
    NotificationsQuery, RemoteStatusNotificationRow, RemoteStatusRecord, RemoteStatusRow,
    actor_url, can_view_local_status, list_local_status_notifications_for_account,
    list_remote_status_notifications_for_account, remote_account_rest_id, remote_status_from_record,
};
use worker::Result;

use crate::D1Database;

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
        text_content: status.text_content.clone(),
        spoiler_text: status.spoiler_text.clone(),
        visibility: status.visibility.clone(),
        sensitive: status.sensitive,
        language: status.language.clone(),
        quote_state: status.quote_state.clone(),
        published_at: status.published_at.clone(),
        edited_at: status.edited_at.clone(),
        card_json: status.card_json.clone(),
        federated_emojis_json: status.federated_emojis_json.clone(),
        in_reply_to_id: status.in_reply_to_id.clone(),
    })
    .ok()
}

pub(crate) use super::status_preload::preload_notification_statuses;

pub(crate) fn build_status_notification_entry(
    id: String,
    notification_type: &str,
    created_at: String,
    account: MastodonAccountResponse,
    status: MastodonStatusResponse,
) -> NotificationEntry {
    let mut entries = Vec::new();
    push_notification_entry(
        &mut entries,
        MastodonNotificationResponse {
            group_key: id.clone(),
            id,
            notification_type: notification_type.to_owned(),
            created_at,
            account,
            status: Some(status),
            report: None,
        },
    );
    entries
        .pop()
        .expect("status notification entry is pushed before it is returned")
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

    let (local_statuses, remote_status_rows) = futures_util::try_join!(
        list_local_status_notifications_for_account(db, viewer.id(), per_type_limit),
        list_remote_status_notifications_for_account(db, viewer.id(), per_type_limit),
    )?;
    let remote_statuses = remote_status_rows
        .iter()
        .filter_map(remote_status_notification_row)
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
                format!("status-local-{}-{}", actor.id(), status.id),
                "status",
                status.created_at,
                MastodonAccountResponse::from_account(&actor, config),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(local_entries);

    let mut remote_candidates = Vec::new();
    for raw_status in remote_status_rows {
        if raw_status.visibility == "direct" {
            continue;
        }
        let Some(status) = remote_status_notification_row(&raw_status) else {
            continue;
        };
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
        remote_candidates.push((raw_status.published_at, status, actor, remote_id));
    }

    let remote_entries = futures_util::future::try_join_all(remote_candidates.into_iter().map(
        |(created_at, status, actor, remote_id)| async move {
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
                format!("status-remote-{}-{}", remote_id, status.id),
                "status",
                created_at,
                MastodonAccountResponse::from_remote_actor(actor),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(remote_entries);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::{QuoteState, Visibility};

    fn test_remote_notification_row() -> RemoteStatusNotificationRow {
        RemoteStatusNotificationRow {
            id: "remote-status-1".to_owned(),
            actor_uri: "https://remote.example/users/bob".to_owned(),
            object_uri: "https://remote.example/users/bob/statuses/1".to_owned(),
            url: Some("https://remote.example/@bob/1".to_owned()),
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hi</p>".to_owned(),
            text_content: "Hi".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 1,
            language: Some("en".to_owned()),
            quote_state: "accepted".to_owned(),
            published_at: "2026-01-02T00:00:00Z".to_owned(),
            edited_at: None,
            card_json: None,
            federated_emojis_json: "[]".to_owned(),
            in_reply_to_id: None,
        }
    }

    #[test]
    fn remote_status_notification_row_maps_valid_row() {
        let status = remote_status_notification_row(&test_remote_notification_row())
            .expect("valid row maps to a remote status");

        assert_eq!(status.id, "remote-status-1");
        assert_eq!(status.actor_uri, "https://remote.example/users/bob");
        assert_eq!(
            status.object_uri,
            "https://remote.example/users/bob/statuses/1"
        );
        assert_eq!(status.url.as_deref(), Some("https://remote.example/@bob/1"));
        assert_eq!(status.content_html, "<p>Hi</p>");
        assert_eq!(status.visibility, Visibility::Public);
        assert!(status.sensitive);
        assert_eq!(status.language.as_deref(), Some("en"));
        assert_eq!(status.quote_state, QuoteState::Accepted);
        assert_eq!(status.published_at, "2026-01-02T00:00:00Z");
    }

    #[test]
    fn remote_status_notification_row_rejects_invalid_visibility() {
        let mut row = test_remote_notification_row();
        row.visibility = "invalid".to_owned();

        assert!(remote_status_notification_row(&row).is_none());
    }
}
