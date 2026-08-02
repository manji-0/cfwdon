use super::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, actor_url,
    build_status_notification_entry, find_account_by_id, find_remote_actor_by_actor_uri,
    find_statuses_by_ids, list_favourite_notifications_for_account,
    list_local_follow_notifications_for_account,
    list_local_follow_request_notifications_for_account,
    list_remote_favourite_notifications_for_account, list_remote_follow_notifications_for_account,
    list_remote_follow_request_notifications_for_account, muted_notifications_for_actor,
    preload_notification_statuses, remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use std::collections::HashMap;
use worker::Result;

use crate::D1Database;
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
        list_local_follow_request_notifications_for_account(db, viewer.id(), per_type_limit).await?
    {
        let Some(account) = find_account_by_id(db, &follow.follower_account_id).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, viewer.id(), &actor_url(config, account.username()))
            .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), account.id(), None)
        {
            continue;
        }
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("follow-request-local-{}", account.id()),
                notification_type: "follow_request".to_owned(),
                group_key: format!("follow-request-local-{}", account.id()),
                created_at: follow.created_at,
                account: MastodonAccountResponse::from_account(&account, config),
                status: None,
                report: None,
            },
        );
    }

    for follow in
        list_remote_follow_request_notifications_for_account(db, viewer.id(), per_type_limit)
            .await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &follow.actor_uri).await? else {
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
        list_local_follow_notifications_for_account(db, viewer.id(), per_type_limit).await?
    {
        let Some(account) = find_account_by_id(db, &follow.follower_account_id).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, viewer.id(), &actor_url(config, account.username()))
            .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), account.id(), None)
        {
            continue;
        }
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: format!("follow-local-{}", account.id()),
                notification_type: "follow".to_owned(),
                group_key: format!("follow-local-{}", account.id()),
                created_at: follow.created_at,
                account: MastodonAccountResponse::from_account(&account, config),
                status: None,
                report: None,
            },
        );
    }

    for follow in
        list_remote_follow_notifications_for_account(db, viewer.id(), per_type_limit).await?
    {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &follow.actor_uri).await? else {
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

    let (local_favourites, remote_favourites) = futures_util::try_join!(
        list_favourite_notifications_for_account(
            db,
            viewer.id(),
            per_type_limit,
            query.min_created_at.as_deref(),
        ),
        list_remote_favourite_notifications_for_account(
            db,
            viewer.id(),
            per_type_limit,
            query.min_created_at.as_deref(),
        ),
    )?;
    let status_ids = local_favourites
        .iter()
        .map(|favourite| favourite.status_id.clone())
        .chain(
            remote_favourites
                .iter()
                .map(|favourite| favourite.status_id.clone()),
        )
        .collect::<Vec<_>>();
    let statuses_by_id = find_statuses_by_ids(db, &status_ids)
        .await?
        .into_iter()
        .map(|status| (status.id.clone(), status))
        .collect::<HashMap<_, _>>();
    let local_actor_ids = local_favourites
        .iter()
        .map(|favourite| favourite.account_id.clone())
        .collect::<Vec<_>>();
    let remote_actor_uris = remote_favourites
        .iter()
        .map(|favourite| favourite.remote_actor_uri.clone())
        .collect::<Vec<_>>();
    let local_statuses = statuses_by_id.values().cloned().collect::<Vec<_>>();
    let preloads = preload_notification_statuses(
        db,
        config,
        viewer,
        &local_statuses,
        &[],
        &local_actor_ids,
        &remote_actor_uris,
    )
    .await?;
    let preloads_ref = &preloads;

    let mut local_candidates = Vec::new();
    for favourite in local_favourites {
        let Some(actor) = preloads.local_accounts_by_id.get(&favourite.account_id) else {
            continue;
        };
        if preloads.is_notification_muted(&actor_url(config, actor.username()))
            || !notification_account_matches_filter(query.account_id.as_deref(), actor.id(), None)
        {
            continue;
        }
        let Some(status) = statuses_by_id.get(&favourite.status_id).cloned() else {
            continue;
        };
        local_candidates.push((favourite.created_at, status, actor.clone()));
    }

    let local_entries = futures_util::future::try_join_all(local_candidates.into_iter().map(
        |(created_at, status, actor)| async move {
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
            Ok::<crate::NotificationEntry, worker::Error>(build_status_notification_entry(
                format!("favourite-local-{}-{}", actor.id(), status.id),
                "favourite",
                created_at,
                MastodonAccountResponse::from_account(&actor, config),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(local_entries);

    let mut remote_candidates = Vec::new();
    for favourite in remote_favourites {
        let Some(actor) = preloads
            .remote_actors_by_uri
            .get(&favourite.remote_actor_uri)
        else {
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
        let Some(status) = statuses_by_id.get(&favourite.status_id).cloned() else {
            continue;
        };
        remote_candidates.push((favourite.created_at, status, actor, remote_id));
    }

    let remote_entries = futures_util::future::try_join_all(remote_candidates.into_iter().map(
        |(created_at, status, actor, remote_id)| async move {
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
            Ok::<crate::NotificationEntry, worker::Error>(build_status_notification_entry(
                format!("favourite-remote-{}-{}", remote_id, status.id),
                "favourite",
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
