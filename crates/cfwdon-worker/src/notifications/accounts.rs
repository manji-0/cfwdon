use super::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, actor_url, build_local_status_response,
    find_account_by_id, find_media_attachments_by_status_id, find_remote_actor_by_actor_uri,
    find_status_by_id, list_favourite_notifications_for_account,
    list_local_follow_notifications_for_account,
    list_local_follow_request_notifications_for_account,
    list_remote_favourite_notifications_for_account, list_remote_follow_notifications_for_account,
    list_remote_follow_request_notifications_for_account, load_in_reply_to_account_id,
    muted_notifications_for_actor, remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Result};

pub(crate) async fn collect_follow_request_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "follow_request") {
        return Ok(());
    }

    for follow in
        list_local_follow_request_notifications_for_account(db, &viewer.id, per_type_limit).await?
    {
        let Some(account) = find_account_by_id(db, &follow.follower_account_id).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &account.username))
            .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), &account.id, None)
        {
            continue;
        }
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("follow-request-local-{}", account.id),
                notification_type: "follow_request".to_owned(),
                group_key: format!("follow-request-local-{}", account.id),
                created_at: follow.created_at,
                account: MastodonAccountResponse::from_account(&account, config),
                status: None,
                report: None,
            },
        );
    }

    for follow in
        list_remote_follow_request_notifications_for_account(db, &viewer.id, per_type_limit).await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &follow.actor_uri).await? else {
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
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("follow-request-remote-{}", remote_id),
                notification_type: "follow_request".to_owned(),
                group_key: format!("follow-request-remote-{}", remote_id),
                created_at: follow.created_at,
                account: MastodonAccountResponse::from_remote_actor(&actor),
                status: None,
                report: None,
            },
        );
    }

    Ok(())
}

pub(crate) async fn collect_follow_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "follow") {
        return Ok(());
    }

    for follow in
        list_local_follow_notifications_for_account(db, &viewer.id, per_type_limit).await?
    {
        let Some(account) = find_account_by_id(db, &follow.follower_account_id).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &account.username))
            .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), &account.id, None)
        {
            continue;
        }
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("follow-local-{}", account.id),
                notification_type: "follow".to_owned(),
                group_key: format!("follow-local-{}", account.id),
                created_at: follow.created_at,
                account: MastodonAccountResponse::from_account(&account, config),
                status: None,
                report: None,
            },
        );
    }

    for follow in
        list_remote_follow_notifications_for_account(db, &viewer.id, per_type_limit).await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &follow.actor_uri).await? else {
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
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("follow-remote-{}", remote_id),
                notification_type: "follow".to_owned(),
                group_key: format!("follow-remote-{}", remote_id),
                created_at: follow.created_at,
                account: MastodonAccountResponse::from_remote_actor(&actor),
                status: None,
                report: None,
            },
        );
    }

    Ok(())
}

pub(crate) async fn collect_favourite_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "favourite") {
        return Ok(());
    }

    for favourite in list_favourite_notifications_for_account(
        db,
        &viewer.id,
        per_type_limit,
        query.min_created_at.as_deref(),
    )
    .await?
    {
        let Some(actor) = find_account_by_id(db, &favourite.account_id).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &actor.username))
            .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), &actor.id, None)
        {
            continue;
        }
        let Some(status) = find_status_by_id(db, &favourite.status_id).await? else {
            continue;
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
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("favourite-local-{}-{}", actor.id, status.id),
                notification_type: "favourite".to_owned(),
                group_key: format!("favourite-local-{}-{}", actor.id, status.id),
                created_at: favourite.created_at,
                account: MastodonAccountResponse::from_account(&actor, config),
                status: Some(status_response),
                report: None,
            },
        );
    }

    for favourite in list_remote_favourite_notifications_for_account(
        db,
        &viewer.id,
        per_type_limit,
        query.min_created_at.as_deref(),
    )
    .await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &favourite.remote_actor_uri).await?
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
        let Some(status) = find_status_by_id(db, &favourite.status_id).await? else {
            continue;
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
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("favourite-remote-{}-{}", remote_id, status.id),
                notification_type: "favourite".to_owned(),
                group_key: format!("favourite-remote-{}-{}", remote_id, status.id),
                created_at: favourite.created_at,
                account: MastodonAccountResponse::from_remote_actor(&actor),
                status: Some(status_response),
                report: None,
            },
        );
    }

    Ok(())
}
