use super::notifications::{
    NotificationEntry, notification_account_matches_filter, notification_type_allowed,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, StatusRow, actor_url,
    build_status_notification_entry, can_view_local_status,
    list_local_quote_notifications_for_account, list_quoted_update_notifications_for_account,
    list_remote_quote_notifications_for_account, notification_timestamp_sort_token,
    preload_notification_statuses, remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use worker::Result;

use crate::D1Database;
pub(crate) async fn collect_quote_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "quote") {
        return Ok(());
    }

    let (local_quotes, remote_quotes) = futures_util::try_join!(
        list_local_quote_notifications_for_account(db, viewer.id(), per_type_limit),
        list_remote_quote_notifications_for_account(db, viewer.id(), per_type_limit),
    )?;
    let preloads =
        preload_notification_statuses(db, config, viewer, &local_quotes, &remote_quotes, &[], &[])
            .await?;
    let preloads_ref = &preloads;

    let mut local_candidates = Vec::new();
    for quote in local_quotes {
        let Some(actor) = preloads.local_accounts_by_id.get(&quote.account_id) else {
            continue;
        };
        if !can_view_local_status(db, &quote, Some(viewer), actor).await?
            || preloads.is_notification_muted(&actor_url(config, actor.username()))
            || !notification_account_matches_filter(query.account_id.as_deref(), actor.id(), None)
        {
            continue;
        }
        local_candidates.push((quote, actor.clone()));
    }

    let local_entries = futures_util::future::try_join_all(local_candidates.into_iter().map(
        |(quote, actor)| async move {
            let status_response = preloads_ref
                .build_local_status_response(
                    db,
                    config,
                    viewer,
                    &quote,
                    &actor,
                    preloads_ref.local_media(&quote.id),
                )
                .await?;
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                format!("quote-local-{}-{}", actor.id(), quote.id),
                "quote",
                quote.created_at,
                MastodonAccountResponse::from_account(&actor, config),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(local_entries);

    let mut remote_candidates = Vec::new();
    for quote in remote_quotes {
        let Some(actor) = preloads.remote_actors_by_uri.get(&quote.actor_uri) else {
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
        remote_candidates.push((quote, actor, remote_id));
    }

    let remote_entries = futures_util::future::try_join_all(remote_candidates.into_iter().map(
        |(quote, actor, remote_id)| async move {
            let status_response = preloads_ref
                .build_remote_status_response(
                    db,
                    config,
                    viewer,
                    &quote,
                    actor,
                    preloads_ref.remote_media(&quote.id),
                )
                .await?;
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                format!("quote-remote-{}-{}", remote_id, quote.id),
                "quote",
                quote.published_at,
                MastodonAccountResponse::from_remote_actor(actor),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(remote_entries);

    Ok(())
}

pub(crate) async fn collect_quoted_update_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "quoted_update") {
        return Ok(());
    }

    let updates =
        list_quoted_update_notifications_for_account(db, viewer.id(), per_type_limit).await?;
    let statuses = updates
        .iter()
        .map(quoted_update_status_row)
        .collect::<Vec<_>>();
    let remote_actor_uris = updates
        .iter()
        .map(|update| update.remote_actor_uri.clone())
        .collect::<Vec<_>>();
    let preloads =
        preload_notification_statuses(db, config, viewer, &statuses, &[], &[], &remote_actor_uris)
            .await?;
    let preloads_ref = &preloads;

    let mut candidates = Vec::new();
    for (update, status) in updates.into_iter().zip(statuses) {
        let Some(actor) = preloads.remote_actors_by_uri.get(&update.remote_actor_uri) else {
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
        let update_token = notification_timestamp_sort_token(&update.remote_updated_at)
            .unwrap_or_else(|| update.remote_updated_at.replace([':', ' '], "-"));
        candidates.push((
            update.remote_updated_at,
            status,
            actor,
            remote_id,
            update_token,
        ));
    }

    let notification_entries = futures_util::future::try_join_all(candidates.into_iter().map(
        |(created_at, status, actor, remote_id, update_token)| async move {
            let status_response = preloads_ref
                .build_local_status_response(
                    db,
                    config,
                    viewer,
                    &status,
                    viewer,
                    preloads_ref.local_media(&status.id),
                )
                .await?;
            let id = format!("quoted-update-{}-{}-{}", remote_id, status.id, update_token);
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                id,
                "quoted_update",
                created_at,
                MastodonAccountResponse::from_remote_actor(actor),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(notification_entries);

    Ok(())
}

use cfwdon_domain::{QuoteState, Visibility};

fn quoted_update_status_row(update: &crate::QuotedUpdateNotificationRow) -> StatusRow {
    StatusRow {
        id: update.id.clone(),
        account_id: update.account_id.clone(),
        ap_id: update.ap_id.clone(),
        in_reply_to_id: update.in_reply_to_id.clone(),
        boost_of_uri: update.boost_of_uri.clone(),
        quote_of_uri: update.quote_of_uri.clone(),
        content_html: update.content_html.clone(),
        text: update.text_content.clone(),
        spoiler_text: update.spoiler_text.clone(),
        visibility: Visibility::parse(&update.visibility).unwrap_or(Visibility::Public),
        sensitive: update.sensitive != 0,
        language: update.language.clone(),
        quote_approval_policy: None,
        quote_state: QuoteState::parse(&update.quote_state).unwrap_or(QuoteState::Accepted),
        application_id: None,
        created_at: update.created_at.clone(),
        updated_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuotedUpdateNotificationRow;

    #[test]
    fn quoted_update_status_row_preserves_status_fields() {
        let update = QuotedUpdateNotificationRow {
            id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: Some("https://example.com/users/alice/statuses/1".to_owned()),
            in_reply_to_id: Some("status-0".to_owned()),
            boost_of_uri: Some("https://remote.example/statuses/boost".to_owned()),
            quote_of_uri: Some("https://remote.example/statuses/quoted".to_owned()),
            content_html: "<p>hello</p>".to_owned(),
            text_content: "hello".to_owned(),
            spoiler_text: "spoiler".to_owned(),
            visibility: "unlisted".to_owned(),
            sensitive: 1,
            language: Some("en".to_owned()),
            quote_state: "accepted".to_owned(),
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            remote_actor_uri: "https://remote.example/users/bob".to_owned(),
            remote_updated_at: "2025-01-02T00:00:00Z".to_owned(),
        };

        let status = quoted_update_status_row(&update);

        assert_eq!(status.id, update.id);
        assert_eq!(status.account_id, update.account_id);
        assert_eq!(status.ap_id, update.ap_id);
        assert_eq!(status.in_reply_to_id, update.in_reply_to_id);
        assert_eq!(status.boost_of_uri, update.boost_of_uri);
        assert_eq!(status.quote_of_uri, update.quote_of_uri);
        assert_eq!(status.content_html, update.content_html);
        assert_eq!(status.text, update.text_content);
        assert_eq!(status.spoiler_text, update.spoiler_text);
        assert_eq!(
            status.visibility,
            Visibility::parse(&update.visibility).unwrap_or(Visibility::Public)
        );
        assert_eq!(status.sensitive, update.sensitive != 0);
        assert_eq!(status.language, update.language);
        assert_eq!(
            status.quote_state,
            QuoteState::parse(&update.quote_state).unwrap_or(QuoteState::Accepted)
        );
        assert_eq!(status.created_at, update.created_at);
        assert!(status.quote_approval_policy.is_none());
        assert!(status.application_id.is_none());
    }
}
