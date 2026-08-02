use super::notifications::{
    NotificationEntry, notification_account_matches_filter, notification_type_allowed,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, StatusRow, actor_url,
    build_status_notification_entry, find_statuses_by_ids, list_reblog_notifications_for_account,
    list_remote_reblog_notifications_for_account, preload_notification_statuses,
    remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use std::collections::HashMap;
use worker::Result;

use crate::D1Database;
pub(crate) async fn collect_reblog_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "reblog") {
        return Ok(());
    }

    let (local_reblogs, remote_reblogs) = futures_util::try_join!(
        list_reblog_notifications_for_account(db, viewer.id(), per_type_limit),
        list_remote_reblog_notifications_for_account(db, viewer.id(), per_type_limit),
    )?;
    let status_ids = local_reblogs
        .iter()
        .map(|reblog| reblog.status_id.clone())
        .chain(remote_reblogs.iter().map(|reblog| reblog.status_id.clone()))
        .collect::<Vec<_>>();
    let statuses_by_id = find_statuses_by_ids(db, &status_ids)
        .await?
        .into_iter()
        .map(|status| (status.id.clone(), status))
        .collect::<HashMap<String, StatusRow>>();
    let local_actor_ids = local_reblogs
        .iter()
        .map(|reblog| reblog.account_id.clone())
        .collect::<Vec<_>>();
    let remote_actor_uris = remote_reblogs
        .iter()
        .map(|reblog| reblog.remote_actor_uri.clone())
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
    for reblog in local_reblogs {
        let Some(status) = statuses_by_id.get(&reblog.status_id).cloned() else {
            continue;
        };
        let Some(actor) = preloads.local_accounts_by_id.get(&reblog.account_id) else {
            continue;
        };
        if preloads.is_notification_muted(&actor_url(config, actor.username()))
            || !notification_account_matches_filter(query.account_id.as_deref(), actor.id(), None)
        {
            continue;
        }
        local_candidates.push((reblog.created_at, status, actor.clone()));
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
                format!("reblog-local-{}-{}", actor.id(), status.id),
                "reblog",
                created_at,
                MastodonAccountResponse::from_account(&actor, config),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(local_entries);

    let mut remote_candidates = Vec::new();
    for reblog in remote_reblogs {
        let Some(status) = statuses_by_id.get(&reblog.status_id).cloned() else {
            continue;
        };
        let Some(actor) = preloads.remote_actors_by_uri.get(&reblog.remote_actor_uri) else {
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
        remote_candidates.push((reblog.created_at, status, actor, remote_id));
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
                format!("reblog-remote-{}-{}", remote_id, status.id),
                "reblog",
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
