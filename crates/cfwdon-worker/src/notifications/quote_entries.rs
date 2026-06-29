use super::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, StatusRow, actor_url,
    build_local_status_response, build_remote_status_response, can_view_local_status,
    find_account_by_id, find_media_attachments_by_status_id, find_remote_actor_by_actor_uri,
    list_local_quote_notifications_for_account, list_quoted_update_notifications_for_account,
    list_remote_quote_notifications_for_account, load_in_reply_to_account_id,
    muted_notifications_for_actor, notification_timestamp_sort_token, remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

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

    for quote in list_local_quote_notifications_for_account(db, viewer.id(), per_type_limit).await?
    {
        let Some(actor) = find_account_by_id(db, &quote.account_id).await? else {
            continue;
        };
        if !can_view_local_status(db, &quote, Some(viewer), &actor).await?
            || muted_notifications_for_actor(db, viewer.id(), &actor_url(config, actor.username()))
                .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), actor.id(), None)
        {
            continue;
        }
        let media = find_media_attachments_by_status_id(db, &quote.id).await?;
        let status_response = build_local_status_response(
            db,
            config,
            Some(viewer),
            &quote,
            &actor,
            load_in_reply_to_account_id(db, &quote).await?,
            media,
        )
        .await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("quote-local-{}-{}", actor.id(), quote.id),
                notification_type: "quote".to_owned(),
                group_key: format!("quote-local-{}-{}", actor.id(), quote.id),
                created_at: quote.created_at.clone(),
                account: MastodonAccountResponse::from_account(&actor, config),
                status: Some(status_response),
                report: None,
            },
        );
    }

    for quote in
        list_remote_quote_notifications_for_account(db, viewer.id(), per_type_limit).await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &quote.actor_uri).await? else {
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
        let status_response =
            build_remote_status_response(db, config, Some(viewer), &quote, &actor).await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("quote-remote-{}-{}", remote_id, quote.id),
                notification_type: "quote".to_owned(),
                group_key: format!("quote-remote-{}-{}", remote_id, quote.id),
                created_at: quote.published_at,
                account: MastodonAccountResponse::from_remote_actor(&actor),
                status: Some(status_response),
                report: None,
            },
        );
    }

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

    for update in
        list_quoted_update_notifications_for_account(db, viewer.id(), per_type_limit).await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &update.remote_actor_uri).await?
        else {
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
        let status = quoted_update_status_row(&update);
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        let status_response = build_local_status_response(
            db,
            config,
            Some(viewer),
            &status,
            viewer,
            load_in_reply_to_account_id(db, &status).await?,
            media,
        )
        .await?;
        let update_token = notification_timestamp_sort_token(&update.remote_updated_at)
            .unwrap_or_else(|| update.remote_updated_at.replace([':', ' '], "-"));
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("quoted-update-{}-{}-{}", remote_id, status.id, update_token),
                notification_type: "quoted_update".to_owned(),
                group_key: format!("quoted-update-{}-{}-{}", remote_id, status.id, update_token),
                created_at: update.remote_updated_at.clone(),
                account: MastodonAccountResponse::from_remote_actor(&actor),
                status: Some(status_response),
                report: None,
            },
        );
    }

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
