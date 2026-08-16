use super::{
    LinkTimelineQuery, PublicTimelineCandidate, PublicTimelineCandidateEntry, candidate_render,
    canonicalize_link_timeline_url, derive_link_timeline_match_urls, empty_timeline_response,
    muted_local_timeline_status_ids, preload_local_timeline_rows, resolve_timeline_cursor,
    resolve_timeline_request_access, select_public_timeline_candidates,
    status_card_url_matches_targets, timeline_cursor_is_unresolved, timeline_cursor_requested,
    timeline_fetch_limit, timeline_invalid_access_token_response, timeline_limit,
    timeline_request_requires_authorization, timeline_response_from_entries,
};
use crate::runtime_config::load_config;
use crate::{
    account_has_thread_mutes, list_local_public_statuses_by_link,
    list_remote_public_statuses_by_link, load_account_filter_matcher, open_bound_request_session,
    with_d1_bookmark,
};
use std::collections::HashSet;
use worker::{Error, Request, Response, Result, RouteContext};

pub(crate) async fn link_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: LinkTimelineQuery = req.query().unwrap_or_default();
    let target_url = query
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing url query parameter".to_owned()))?;
    let target_urls = derive_link_timeline_match_urls(target_url);
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
    let query_limit = timeline_fetch_limit(limit);
    let (session, db) = open_bound_request_session(&ctx, &config, &req)?;
    let access = resolve_timeline_request_access(&req, &db, &config).await?;
    if timeline_request_requires_authorization(
        true,
        true,
        config.timeline_trending_link_feeds_local,
        config.timeline_trending_link_feeds_remote,
    ) && !access.is_authorized()
    {
        return timeline_invalid_access_token_response();
    }
    if !crate::trending_link_target_is_known(&db, &target_urls).await? {
        return Response::error("Record not found", 404);
    }
    let target_url_set = target_urls
        .iter()
        .filter_map(|url| canonicalize_link_timeline_url(url))
        .collect::<HashSet<_>>();
    let viewer = access.viewer();
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;
    if timeline_cursor_is_unresolved(&pagination, &cursor) {
        return with_d1_bookmark(empty_timeline_response()?, &session);
    }
    let filter_matcher = match viewer {
        Some(viewer) => Some(load_account_filter_matcher(&db, viewer.id()).await?),
        None => None,
    };

    let local_link_statuses =
        list_local_public_statuses_by_link(&db, &target_urls, &cursor, query_limit);
    let remote_link_statuses =
        list_remote_public_statuses_by_link(&db, &target_urls, &cursor, query_limit);
    let (local_link_statuses, remote_link_statuses) =
        futures_util::try_join!(local_link_statuses, remote_link_statuses)?;
    let (local_accounts_by_id, mut media_by_status_id) =
        preload_local_timeline_rows(&db, &local_link_statuses).await?;
    let viewer_has_thread_mutes = match viewer {
        Some(viewer) => account_has_thread_mutes(&db, viewer.id()).await?,
        None => false,
    };
    let muted_local_status_ids = match viewer {
        Some(viewer) => {
            muted_local_timeline_status_ids(
                &db,
                viewer.id(),
                viewer_has_thread_mutes,
                &local_link_statuses.iter().collect::<Vec<_>>(),
            )
            .await?
        }
        None => HashSet::new(),
    };
    let mut candidates = Vec::new();

    for status in local_link_statuses {
        if !status_card_url_matches_targets(&status.text, &target_url_set) {
            continue;
        }
        if !local_accounts_by_id.contains_key(&status.account_id) {
            continue;
        };
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

    for (status, actor) in remote_link_statuses {
        if !status_card_url_matches_targets(&status.plain_text(), &target_url_set) {
            continue;
        }
        candidates.push(PublicTimelineCandidateEntry {
            timestamp: status.published_at.clone(),
            id: status.id.clone(),
            candidate: PublicTimelineCandidate::Remote { status, actor },
        });
    }

    let candidates = select_public_timeline_candidates(candidates, limit);
    if candidates.is_empty() && !timeline_cursor_requested(&pagination) {
        return Response::error("Record not found", 404);
    }
    let entries = candidate_render::timeline_entries_from_candidates(
        &db,
        &config,
        viewer,
        filter_matcher.as_ref(),
        &local_accounts_by_id,
        candidates,
        true,
        Some(viewer_has_thread_mutes),
    )
    .await?;

    with_d1_bookmark(
        timeline_response_from_entries(&req, limit, entries)?,
        &session,
    )
}
