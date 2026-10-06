use super::{
    NotificationsQuery, build_status_notification_entry, list_favourite_notifications_for_account,
    list_local_follow_notifications_for_account,
    list_local_follow_request_notifications_for_account,
    list_remote_favourite_notifications_for_account, list_remote_follow_notifications_for_account,
    list_remote_follow_request_notifications_for_account, notification_time_window,
    preload_notification_statuses,
};
use crate::accounts::find_accounts_by_ids;
use crate::identity::{actor_url, remote_account_rest_id};
use crate::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use crate::responses::MastodonAccountResponse;
use crate::statuses::find_statuses_by_ids;
use crate::store::relationship::list_notification_muted_actor_uris_for_account;
use crate::store::remote::find_remote_actors_by_actor_uris;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use std::collections::HashMap;
use worker::Result;
/// Account-only notification rows (follow, follow request) before hydration.
struct AccountNotificationRows {
    /// `(follower account id, created_at)`
    local: Vec<(String, String)>,
    /// `(follower actor uri, created_at)`
    remote: Vec<(String, String)>,
}

/// Hydrate every row with one batched account, actor, and mute lookup instead
/// of three serial queries per row.
#[allow(clippy::too_many_arguments)]
async fn push_account_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    notification_type: &str,
    id_prefix: &str,
    rows: AccountNotificationRows,
) -> Result<()> {
    let local_ids = rows
        .local
        .iter()
        .map(|(account_id, _)| account_id.clone())
        .collect::<Vec<_>>();
    let remote_uris = rows
        .remote
        .iter()
        .map(|(actor_uri, _)| actor_uri.clone())
        .collect::<Vec<_>>();
    let (accounts_by_id, actors_by_uri, muted_actor_uris) = futures_util::try_join!(
        find_accounts_by_ids(db, &local_ids),
        find_remote_actors_by_actor_uris(db, &remote_uris),
        list_notification_muted_actor_uris_for_account(db, viewer.id()),
    )?;

    for (account_id, created_at) in rows.local {
        let Some(account) = accounts_by_id.get(&account_id) else {
            continue;
        };
        if muted_actor_uris.contains(&actor_url(config, account.username()))
            || !notification_account_matches_filter(query.account_id.as_deref(), account.id(), None)
        {
            continue;
        }
        let id = format!("{id_prefix}-local-{}", account.id());
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: id.clone(),
                notification_type: notification_type.to_owned(),
                group_key: id,
                created_at,
                account: MastodonAccountResponse::from_account(account, config),
                status: None,
                report: None,
            },
        );
    }

    for (actor_uri, created_at) in rows.remote {
        let Some(actor) = actors_by_uri.get(&actor_uri) else {
            continue;
        };
        if muted_actor_uris.contains(&actor.actor_uri) {
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
        let id = format!("{id_prefix}-remote-{remote_id}");
        push_notification_entry(
            entries,
            MastodonNotificationResponse {
                id: id.clone(),
                notification_type: notification_type.to_owned(),
                group_key: id,
                created_at,
                account: MastodonAccountResponse::from_remote_actor(actor),
                status: None,
                report: None,
            },
        );
    }

    Ok(())
}

pub(crate) async fn collect_follow_request_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    let window = notification_time_window(query);
    if !notification_type_allowed(query, "follow_request") {
        return Ok(());
    }

    let (local, remote) = futures_util::try_join!(
        list_local_follow_request_notifications_for_account(
            db,
            viewer.id(),
            per_type_limit,
            &window
        ),
        list_remote_follow_request_notifications_for_account(
            db,
            viewer.id(),
            per_type_limit,
            &window
        ),
    )?;
    let rows = AccountNotificationRows {
        local: local
            .into_iter()
            .map(|row| (row.follower_account_id, row.created_at))
            .collect(),
        remote: remote
            .into_iter()
            .map(|row| (row.actor_uri, row.created_at))
            .collect(),
    };
    push_account_notification_entries(
        entries,
        db,
        config,
        viewer,
        query,
        "follow_request",
        "follow-request",
        rows,
    )
    .await
}

pub(crate) async fn collect_follow_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    let window = notification_time_window(query);
    if !notification_type_allowed(query, "follow") {
        return Ok(());
    }

    let (local, remote) = futures_util::try_join!(
        list_local_follow_notifications_for_account(db, viewer.id(), per_type_limit, &window),
        list_remote_follow_notifications_for_account(db, viewer.id(), per_type_limit, &window),
    )?;
    let rows = AccountNotificationRows {
        local: local
            .into_iter()
            .map(|row| (row.follower_account_id, row.created_at))
            .collect(),
        remote: remote
            .into_iter()
            .map(|row| (row.actor_uri, row.created_at))
            .collect(),
    };
    push_account_notification_entries(entries, db, config, viewer, query, "follow", "follow", rows)
        .await
}

pub(crate) async fn collect_favourite_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    let window = notification_time_window(query);
    if !notification_type_allowed(query, "favourite") {
        return Ok(());
    }

    let (local_favourites, remote_favourites) = futures_util::try_join!(
        list_favourite_notifications_for_account(db, viewer.id(), per_type_limit, &window,),
        list_remote_favourite_notifications_for_account(db, viewer.id(), per_type_limit, &window,),
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
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
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
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
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
