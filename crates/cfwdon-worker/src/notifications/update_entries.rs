use super::notifications::{
    NotificationEntry, notification_account_matches_filter, notification_type_allowed,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, build_status_notification_entry,
    list_update_notifications_for_account, notification_timestamp_sort_token,
    preload_notification_statuses, remote_account_rest_id,
};
use cfwdon_domain::LocalAccount;
use worker::Result;

use crate::D1Database;
pub(crate) async fn collect_update_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "update") {
        return Ok(());
    }

    let updates = list_update_notifications_for_account(db, viewer.id(), per_type_limit).await?;
    let mut status_updates = Vec::new();
    for update in updates {
        let Ok(status) = update.as_remote_status_row() else {
            continue;
        };
        status_updates.push((update, status));
    }
    let remote_statuses = status_updates
        .iter()
        .map(|(_, status)| status.clone())
        .collect::<Vec<_>>();
    let preloads =
        preload_notification_statuses(db, config, viewer, &[], &remote_statuses, &[], &[]).await?;
    let preloads_ref = &preloads;

    let mut candidates = Vec::new();
    for (update, status) in status_updates {
        let Some(actor) = preloads.remote_actors_by_uri.get(&update.actor_uri) else {
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
                .build_remote_status_response(
                    db,
                    config,
                    viewer,
                    &status,
                    actor,
                    preloads_ref.remote_media(&status.id),
                )
                .await?;
            let id = format!("update-remote-{}-{}-{}", remote_id, status.id, update_token);
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                id,
                "update",
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
