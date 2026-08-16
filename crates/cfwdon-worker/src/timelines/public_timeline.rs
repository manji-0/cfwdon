use super::{
    PublicTimelineCandidate, PublicTimelineCandidateEntry, PublicTimelineQuery, TimelineEntry,
    build_timeline_link_header, candidate_render, empty_timeline_response, include_local_source,
    include_remote_source, muted_local_timeline_status_ids, preload_local_timeline_rows,
    remote_media_status_ids_for_filter, resolve_timeline_cursor, resolve_timeline_request_access,
    select_public_timeline_candidates, timeline_cursor_is_unresolved, timeline_cursor_requested,
    timeline_fetch_limit, timeline_invalid_access_token_response, timeline_limit,
    timeline_request_requires_authorization, timeline_response_from_entries,
};
use crate::runtime_config::load_config;
use crate::{
    D1Database, ResolvedTimelineCursor, account_has_thread_mutes,
    list_local_public_timeline_statuses, list_remote_public_timeline_statuses,
    load_account_filter_matcher, open_bound_request_session, with_d1_bookmark,
};
use std::collections::HashSet;
use worker::{Request, Response, Result, RouteContext};

/// First-page anonymous federated public timeline — safe to serve from D1 cache.
pub(crate) fn public_timeline_first_page_cacheable(
    query: &PublicTimelineQuery,
    has_viewer: bool,
) -> bool {
    if has_viewer {
        return false;
    }
    if timeline_cursor_requested(&query.pagination()) {
        return false;
    }
    if query.only_media.unwrap_or(false) {
        return false;
    }
    include_local_source(query.local, query.remote)
        && include_remote_source(query.local, query.remote)
}

pub(crate) async fn public_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let Some(response) = crate::d1_pressure_load_shed_response()? {
        return Ok(response);
    }

    let config = load_config(&ctx);
    let query: PublicTimelineQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
    let include_local = include_local_source(query.local, query.remote);
    let include_remote = include_remote_source(query.local, query.remote);
    let (session, db) = open_bound_request_session(&ctx, &config, &req)?;
    let access = resolve_timeline_request_access(&req, &db, &config).await?;
    if timeline_request_requires_authorization(
        include_local,
        include_remote,
        config.timeline_live_feeds_local,
        config.timeline_live_feeds_remote,
    ) && !access.is_authorized()
    {
        return timeline_invalid_access_token_response();
    }
    let viewer = access.viewer();
    let cacheable = public_timeline_first_page_cacheable(&query, viewer.is_some());

    if cacheable
        && let Some(cached) =
            crate::load_public_endpoint_cache(&db, crate::PUBLIC_CACHE_PUBLIC_TIMELINE).await?
    {
        return with_d1_bookmark(
            timeline_response_from_cached_statuses(&req, limit, cached)?,
            &session,
        );
    }

    let cursor = resolve_timeline_cursor(&db, &pagination).await?;
    if timeline_cursor_is_unresolved(&pagination, &cursor) {
        return with_d1_bookmark(empty_timeline_response()?, &session);
    }

    let fetch_limit = if cacheable {
        crate::PUBLIC_TIMELINE_CACHE_SIZE
    } else {
        timeline_fetch_limit(limit)
    };
    let only_media = query.only_media.unwrap_or(false);
    let filter_matcher = match viewer {
        Some(viewer) => Some(load_account_filter_matcher(&db, viewer.id()).await?),
        None => None,
    };
    let viewer_has_thread_mutes = match viewer {
        Some(viewer) => account_has_thread_mutes(&db, viewer.id()).await?,
        None => false,
    };

    let entries = build_public_timeline_entries(
        &db,
        &config,
        &cursor,
        viewer,
        filter_matcher.as_ref(),
        include_local,
        include_remote,
        only_media,
        fetch_limit,
        if cacheable {
            crate::PUBLIC_TIMELINE_CACHE_SIZE
        } else {
            limit
        },
        viewer_has_thread_mutes,
    )
    .await?;

    if cacheable {
        let payload = serde_json::Value::Array(
            entries
                .iter()
                .take(crate::PUBLIC_TIMELINE_CACHE_SIZE as usize)
                .map(|(_, _, value)| value.clone())
                .collect(),
        );
        let _ =
            crate::store_public_endpoint_cache(&db, crate::PUBLIC_CACHE_PUBLIC_TIMELINE, &payload)
                .await;
    }

    with_d1_bookmark(
        timeline_response_from_entries(&req, limit, entries)?,
        &session,
    )
}

pub(crate) async fn refresh_public_timeline_cache(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<()> {
    let cursor = ResolvedTimelineCursor::default();
    let entries = build_public_timeline_entries(
        db,
        config,
        &cursor,
        None,
        None,
        true,
        true,
        false,
        crate::PUBLIC_TIMELINE_CACHE_SIZE,
        crate::PUBLIC_TIMELINE_CACHE_SIZE,
        false,
    )
    .await?;
    let payload = serde_json::Value::Array(
        entries
            .into_iter()
            .take(crate::PUBLIC_TIMELINE_CACHE_SIZE as usize)
            .map(|(_, _, value)| value)
            .collect(),
    );
    crate::store_public_endpoint_cache(db, crate::PUBLIC_CACHE_PUBLIC_TIMELINE, &payload).await
}

async fn build_public_timeline_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    cursor: &crate::ResolvedTimelineCursor,
    viewer: Option<&crate::LocalAccount>,
    filter_matcher: Option<&crate::AccountFilterMatcher>,
    include_local: bool,
    include_remote: bool,
    only_media: bool,
    query_limit: u32,
    select_limit: u32,
    viewer_has_thread_mutes: bool,
) -> Result<Vec<TimelineEntry>> {
    let (local_statuses, remote_statuses) = futures_util::try_join!(
        async {
            if include_local {
                list_local_public_timeline_statuses(db, cursor, query_limit).await
            } else {
                Ok(Vec::new())
            }
        },
        async {
            if include_remote {
                list_remote_public_timeline_statuses(db, cursor, query_limit).await
            } else {
                Ok(Vec::new())
            }
        },
    )?;
    let mut candidates = Vec::new();
    let mut local_accounts_by_id = std::collections::HashMap::new();

    if include_local {
        let (accounts_by_id, mut media_by_status_id) =
            preload_local_timeline_rows(db, &local_statuses).await?;
        local_accounts_by_id = accounts_by_id;
        let muted_local_status_ids = match viewer {
            Some(viewer) => {
                muted_local_timeline_status_ids(
                    db,
                    viewer.id(),
                    viewer_has_thread_mutes,
                    &local_statuses.iter().collect::<Vec<_>>(),
                )
                .await?
            }
            None => HashSet::new(),
        };
        for status in local_statuses {
            if !local_accounts_by_id.contains_key(&status.account_id) {
                continue;
            }
            if muted_local_status_ids.contains(&status.id) {
                continue;
            }
            let media = media_by_status_id.remove(&status.id).unwrap_or_default();
            if only_media && media.is_empty() {
                continue;
            }
            candidates.push(PublicTimelineCandidateEntry {
                timestamp: status.created_at.clone(),
                id: status.id.clone(),
                candidate: PublicTimelineCandidate::Local { status, media },
            });
        }
    }

    if include_remote {
        let remote_media_status_ids =
            remote_media_status_ids_for_filter(db, only_media, &remote_statuses).await?;
        for (status, actor) in remote_statuses {
            if only_media && !remote_media_status_ids.contains(&status.id) {
                continue;
            }
            candidates.push(PublicTimelineCandidateEntry {
                timestamp: status.published_at.clone(),
                id: status.id.clone(),
                candidate: PublicTimelineCandidate::Remote { status, actor },
            });
        }
    }

    let candidates = select_public_timeline_candidates(candidates, select_limit);
    candidate_render::timeline_entries_from_candidates(
        db,
        config,
        viewer,
        filter_matcher,
        &local_accounts_by_id,
        candidates,
        false,
        Some(viewer_has_thread_mutes),
    )
    .await
}

fn timeline_response_from_cached_statuses(
    req: &Request,
    limit: u32,
    payload: serde_json::Value,
) -> Result<Response> {
    let full_len = match &payload {
        serde_json::Value::Array(items) => items.len(),
        _ => 1,
    };
    let page = crate::slice_json_array_cache(payload, 0, limit);
    let has_next_page = full_len > limit as usize;
    let first_id = page
        .first()
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    let last_id = has_next_page
        .then(|| {
            page.last()
                .and_then(|value| value.get("id"))
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        })
        .flatten();
    let mut builder = Response::from_json(&page)?;
    if let Some(link) =
        build_timeline_link_header(req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}
