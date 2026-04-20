use crate::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use crate::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, RemoteStatusRow, StatusRow, actor_url,
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

    for quote in list_local_quote_notifications_for_account(db, &viewer.id, per_type_limit).await? {
        let Some(actor) = find_account_by_id(db, &quote.account_id).await? else {
            continue;
        };
        if !can_view_local_status(db, &quote, Some(viewer), &actor).await?
            || muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &actor.username))
                .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), &actor.id, None)
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
                id: format!("quote-local-{}-{}", actor.id, quote.id),
                notification_type: "quote".to_owned(),
                group_key: format!("quote-local-{}-{}", actor.id, quote.id),
                created_at: quote.created_at.clone(),
                account: MastodonAccountResponse::from_account(&actor, config),
                status: Some(status_response),
                report: None,
            },
        );
    }

    for quote in list_remote_quote_notifications_for_account(db, &viewer.id, per_type_limit).await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &quote.actor_uri).await? else {
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
            id: quote.id.clone(),
            actor_uri: quote.actor_uri.clone(),
            object_uri: quote.object_uri.clone(),
            url: quote.url.clone(),
            in_reply_to_uri: quote.in_reply_to_uri.clone(),
            boost_of_uri: quote.boost_of_uri.clone(),
            quote_of_uri: quote.quote_of_uri.clone(),
            content_html: quote.content_html.clone(),
            spoiler_text: quote.spoiler_text.clone(),
            visibility: quote.visibility.clone(),
            sensitive: quote.sensitive,
            language: quote.language.clone(),
            quote_state: quote.quote_state.clone(),
            published_at: quote.published_at.clone(),
        };
        let status_response =
            build_remote_status_response(db, config, Some(viewer), &status, &actor).await?;
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("quote-remote-{}-{}", remote_id, status.id),
                notification_type: "quote".to_owned(),
                group_key: format!("quote-remote-{}-{}", remote_id, status.id),
                created_at: status.published_at,
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
        list_quoted_update_notifications_for_account(db, &viewer.id, per_type_limit).await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &update.remote_actor_uri).await?
        else {
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
        let status = StatusRow {
            id: update.id.clone(),
            account_id: update.account_id.clone(),
            ap_id: update.ap_id.clone(),
            in_reply_to_id: update.in_reply_to_id.clone(),
            boost_of_uri: update.boost_of_uri.clone(),
            quote_of_uri: update.quote_of_uri.clone(),
            content_html: update.content_html.clone(),
            _text_content: update.text_content.clone(),
            spoiler_text: update.spoiler_text.clone(),
            visibility: update.visibility.clone(),
            sensitive: update.sensitive,
            language: update.language.clone(),
            quote_approval_policy: None,
            quote_state: update.quote_state.clone(),
            created_at: update.created_at.clone(),
        };
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
