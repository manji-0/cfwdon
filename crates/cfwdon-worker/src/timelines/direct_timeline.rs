use super::{
    PublicTimelineCandidate, PublicTimelineCandidateEntry, TimelinePaginationQuery,
    candidate_render, local_status_actor_uri, muted_local_timeline_status_ids,
    preload_local_timeline_rows_from_status_refs, resolve_timeline_cursor,
    select_public_timeline_candidates, timeline_fetch_limit, timeline_limit,
    timeline_response_from_entries,
};
use crate::instance::instance_host;
use crate::runtime_config::load_config;
use crate::{
    account_has_thread_mutes, list_active_muted_actor_uris, list_local_direct_timeline_statuses,
    list_remote_direct_statuses_mentioning_viewer, load_account_filter_matcher,
    open_bound_request_session, require_authenticated_local_account, with_d1_bookmark,
};
use std::collections::{HashMap, HashSet};
use worker::{Request, Response, Result, RouteContext};

async fn preload_muted_timeline_actor_uris(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    local_statuses: &[&crate::StatusRow],
    remote_statuses: &[&(crate::RemoteStatusRow, crate::RemoteActorRow)],
    accounts_by_id: &HashMap<String, crate::LocalAccount>,
) -> Result<HashSet<String>> {
    let mut actor_uris = local_statuses
        .iter()
        .filter_map(|status| local_status_actor_uri(config, accounts_by_id, status))
        .collect::<Vec<_>>();
    actor_uris.extend(
        remote_statuses
            .iter()
            .map(|(_, actor)| actor.actor_uri.clone()),
    );

    list_active_muted_actor_uris(db, viewer.id(), &actor_uris).await
}

pub(crate) async fn direct_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: TimelinePaginationQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query);
    let query_limit = timeline_fetch_limit(limit);
    let (session, db) = open_bound_request_session(&ctx, &config, &req)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let cursor = resolve_timeline_cursor(&db, &query).await?;
    let filter_matcher = load_account_filter_matcher(&db, viewer.id()).await?;
    let mention_pattern = format!(
        "%@{}@{}%",
        viewer.username().to_ascii_lowercase(),
        instance_host(&config)
    );
    let (direct_statuses, remote_direct_rows) = futures_util::try_join!(
        list_local_direct_timeline_statuses(&db, viewer.id(), &cursor, query_limit),
        list_remote_direct_statuses_mentioning_viewer(&db, &mention_pattern, &cursor, query_limit,),
    )?;
    let remote_direct_rows = remote_direct_rows
        .into_iter()
        .filter(|(status, _)| {
            let text = status.plain_text();
            crate::extract_mentions_from_text(&text, &config)
                .into_iter()
                .any(|handle| handle.username == viewer.username())
        })
        .collect::<Vec<_>>();
    let direct_status_refs = direct_statuses.iter().collect::<Vec<_>>();
    let remote_status_refs = remote_direct_rows.iter().collect::<Vec<_>>();
    let (local_accounts_by_id, mut media_by_status_id) =
        preload_local_timeline_rows_from_status_refs(&db, &direct_status_refs).await?;
    let muted_actor_uris = preload_muted_timeline_actor_uris(
        &db,
        &config,
        &viewer,
        &direct_status_refs,
        &remote_status_refs,
        &local_accounts_by_id,
    )
    .await?;
    let viewer_has_thread_mutes = account_has_thread_mutes(&db, viewer.id()).await?;
    let muted_local_status_ids = muted_local_timeline_status_ids(
        &db,
        viewer.id(),
        viewer_has_thread_mutes,
        &direct_status_refs,
    )
    .await?;
    let mut candidates = Vec::new();

    for status in direct_statuses {
        let Some(actor_uri) = local_status_actor_uri(&config, &local_accounts_by_id, &status)
        else {
            continue;
        };
        if muted_actor_uris.contains(&actor_uri) {
            continue;
        }
        if muted_local_status_ids.contains(&status.id) {
            continue;
        }
        let media = media_by_status_id.remove(&status.id).unwrap_or_default();
        candidates.push(PublicTimelineCandidateEntry {
            timestamp: status.created_at.clone(),
            id: status.id.clone(),
            candidate: PublicTimelineCandidate::Local { status, media },
        });
    }

    for (status, actor) in remote_direct_rows {
        if muted_actor_uris.contains(&actor.actor_uri) {
            continue;
        }
        candidates.push(PublicTimelineCandidateEntry {
            timestamp: status.published_at.clone(),
            id: status.id.clone(),
            candidate: PublicTimelineCandidate::Remote { status, actor },
        });
    }

    let candidates = select_public_timeline_candidates(candidates, limit);
    let entries = candidate_render::timeline_entries_from_candidates(
        &db,
        &config,
        Some(&viewer),
        Some(&filter_matcher),
        &local_accounts_by_id,
        candidates,
        false,
        Some(viewer_has_thread_mutes),
    )
    .await?;

    with_d1_bookmark(
        timeline_response_from_entries(&req, limit, entries)?,
        &session,
    )
}
