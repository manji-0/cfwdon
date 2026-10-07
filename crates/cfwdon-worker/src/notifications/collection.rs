use super::{
    NotificationCandidate, NotificationsQuery, collect_admin_report_notifications_entries,
    collect_admin_sign_up_notifications_entries, collect_favourite_notification_candidates,
    collect_follow_notification_candidates, collect_follow_request_notification_candidates,
    collect_mention_notification_candidates, collect_poll_notification_candidates,
    collect_quote_notification_candidates, collect_quoted_update_notification_candidates,
    collect_reblog_notification_candidates, collect_status_notification_candidates,
    collect_update_notification_candidates,
};
use crate::collections_alpha::collect_collection_notification_entries;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::Result;
/// A collector that still renders its own entries, wrapped as ready candidates.
macro_rules! collect_notification_batch {
    ($collector:ident, $db:expr, $config:expr, $viewer:expr, $query:expr, $per_type_limit:expr) => {
        async {
            let mut entries = Vec::new();
            $collector(&mut entries, $db, $config, $viewer, $query, $per_type_limit).await?;
            Ok::<Vec<NotificationCandidate>, worker::Error>(
                entries
                    .into_iter()
                    .map(NotificationCandidate::ready)
                    .collect(),
            )
        }
    };
}

/// A collector that returns candidates.
macro_rules! collect_notification_candidate_batch {
    ($collector:ident, $db:expr, $config:expr, $viewer:expr, $query:expr, $per_type_limit:expr) => {
        async {
            let mut candidates = Vec::new();
            $collector(
                &mut candidates,
                $db,
                $config,
                $viewer,
                $query,
                $per_type_limit,
            )
            .await?;
            Ok::<Vec<NotificationCandidate>, worker::Error>(candidates)
        }
    };
}

pub(crate) async fn collect_notifications(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationCandidate>> {
    let batches = futures_util::try_join!(
        collect_notification_batch!(
            collect_admin_report_notifications_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_admin_sign_up_notifications_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_follow_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_follow_request_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_batch!(
            collect_collection_notification_entries,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_favourite_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_mention_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_quote_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_update_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_quoted_update_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_status_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_poll_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
        collect_notification_candidate_batch!(
            collect_reblog_notification_candidates,
            db,
            config,
            viewer,
            query,
            per_type_limit
        ),
    )?;

    Ok(merge_notification_batches([
        batches.0, batches.1, batches.2, batches.3, batches.4, batches.5, batches.6, batches.7,
        batches.8, batches.9, batches.10, batches.11, batches.12,
    ]))
}

fn merge_notification_batches<const N: usize>(
    batches: [Vec<NotificationCandidate>; N],
) -> Vec<NotificationCandidate> {
    let mut entries = Vec::new();
    for batch in batches {
        entries.extend(batch);
    }
    entries
}
