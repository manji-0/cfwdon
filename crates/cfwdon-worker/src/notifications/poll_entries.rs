use super::notifications::{
    NotificationEntry, notification_account_matches_filter, notification_type_allowed,
};
use super::{
    AppConfig, MastodonAccountResponse, NotificationsQuery, StatusRow, actor_url,
    build_status_notification_entry, find_statuses_by_ids, list_poll_notifications_for_account,
    preload_notification_statuses,
};
use cfwdon_domain::LocalAccount;
use std::collections::HashMap;
use worker::{D1Database, Result};

pub(crate) async fn collect_poll_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "poll") {
        return Ok(());
    }

    let polls = list_poll_notifications_for_account(db, viewer.id(), per_type_limit).await?;
    let status_ids = polls
        .iter()
        .map(|poll| poll.status_id.clone())
        .collect::<Vec<_>>();
    let statuses_by_id = find_statuses_by_ids(db, &status_ids)
        .await?
        .into_iter()
        .map(|status| (status.id.clone(), status))
        .collect::<HashMap<String, StatusRow>>();
    let local_statuses = statuses_by_id.values().cloned().collect::<Vec<_>>();
    let local_actor_ids = polls
        .iter()
        .map(|poll| poll.account_id.clone())
        .collect::<Vec<_>>();
    let preloads = preload_notification_statuses(
        db,
        config,
        viewer,
        &local_statuses,
        &[],
        &local_actor_ids,
        &[],
    )
    .await?;
    let preloads_ref = &preloads;

    let mut candidates = Vec::new();
    for poll in polls {
        let Some(status) = statuses_by_id.get(&poll.status_id).cloned() else {
            continue;
        };
        let Some(actor) = preloads.local_accounts_by_id.get(&poll.account_id) else {
            continue;
        };
        if preloads.is_notification_muted(&actor_url(config, actor.username()))
            || !notification_account_matches_filter(query.account_id.as_deref(), actor.id(), None)
            || !crate::can_view_local_status(db, &status, Some(viewer), actor).await?
        {
            continue;
        }
        candidates.push((poll.poll_id, poll.expires_at, status, actor.clone()));
    }

    let notification_entries = futures_util::future::try_join_all(candidates.into_iter().map(
        |(poll_id, expires_at, status, actor)| async move {
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
            let id = format!("poll-local-{}", poll_id);
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                id,
                "poll",
                expires_at,
                MastodonAccountResponse::from_account(&actor, config),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(notification_entries);

    Ok(())
}
