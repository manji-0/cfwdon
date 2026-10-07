use super::{
    NotificationCandidate, NotificationsQuery, build_status_notification_entry,
    list_local_quote_notifications_for_account, list_quoted_update_notifications_for_account,
    list_remote_quote_notifications_for_account, notification_time_window,
    notification_timestamp_sort_token, preload_notification_statuses,
};
use crate::identity::remote_account_rest_id;
use crate::notifications::{
    NotificationEntry, QuotedUpdateNotificationRow, notification_account_matches_filter,
    notification_type_allowed,
};
use crate::responses::MastodonAccountResponse;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, LocalStatus};
use worker::Result;
pub(crate) async fn collect_quote_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    _config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "quote") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let (local_quotes, remote_quotes) = futures_util::try_join!(
        list_local_quote_notifications_for_account(db, viewer.id(), per_type_limit, &window),
        list_remote_quote_notifications_for_account(db, viewer.id(), per_type_limit, &window),
    )?;
    candidates.extend(
        local_quotes
            .into_iter()
            .map(|status| NotificationCandidate::authored_local_status("quote", status)),
    );
    candidates.extend(
        remote_quotes
            .into_iter()
            .map(|status| NotificationCandidate::authored_remote_status("quote", status)),
    );
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
    let window = notification_time_window(query);
    if !notification_type_allowed(query, "quoted_update") {
        return Ok(());
    }

    let updates =
        list_quoted_update_notifications_for_account(db, viewer.id(), per_type_limit, &window)
            .await?;
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

fn quoted_update_status_row(update: &QuotedUpdateNotificationRow) -> LocalStatus {
    LocalStatus {
        id: update.id.clone(),
        account_id: update.account_id.clone(),
        ap_id: update.ap_id.clone(),
        in_reply_to_id: update.in_reply_to_id.clone(),
        in_reply_to_account_id: update.in_reply_to_account_id.clone(),
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
        card_json: None,
        created_at: update.created_at.clone(),
        updated_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::QuotedUpdateNotificationRow;

    #[test]
    fn quoted_update_status_row_preserves_status_fields() {
        let update = QuotedUpdateNotificationRow {
            id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: Some("https://example.com/users/alice/statuses/1".to_owned()),
            in_reply_to_id: Some("status-0".to_owned()),
            in_reply_to_account_id: None,
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
