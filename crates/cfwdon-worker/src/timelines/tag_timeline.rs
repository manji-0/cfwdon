use super::{
    PublicTimelineCandidate, PublicTimelineCandidateEntry, TagTimelineQuery, candidate_render,
    empty_timeline_response, include_local_source, include_remote_source,
    matches_tag_timeline_filters, muted_local_timeline_status_ids, preload_local_timeline_rows,
    remote_media_status_ids_for_filter, resolve_timeline_cursor, resolve_timeline_request_access,
    select_public_timeline_candidates, timeline_cursor_is_unresolved, timeline_fetch_limit,
    timeline_invalid_access_token_response, timeline_limit,
    timeline_request_requires_authorization, timeline_response_from_entries,
};
use crate::content_helpers::{extract_hashtags_from_html, extract_hashtags_from_text};
use crate::runtime_config::load_config;
use crate::{
    account_has_thread_mutes, list_local_public_statuses_by_tag,
    list_remote_public_statuses_by_tag, load_account_filter_matcher, normalize_hashtag,
    open_bound_request_session, with_d1_bookmark,
};
use std::collections::HashSet;
use worker::{Error, Request, Response, Result, RouteContext};

pub(crate) async fn tag_timeline_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("hashtag")
        .map(|value| normalize_hashtag(value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing hashtag route parameter".to_owned()))?;
    let query: TagTimelineQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
    let query_limit = timeline_fetch_limit(limit);
    let include_local = include_local_source(query.local, query.remote);
    let include_remote = include_remote_source(query.local, query.remote);
    let (session, db) = open_bound_request_session(&ctx, &config, &req)?;
    let access = resolve_timeline_request_access(&req, &db, &config).await?;
    if timeline_request_requires_authorization(
        include_local,
        include_remote,
        config.timeline_hashtag_feeds_local,
        config.timeline_hashtag_feeds_remote,
    ) && !access.is_authorized()
    {
        return timeline_invalid_access_token_response();
    }
    let viewer = access.viewer();
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;
    if timeline_cursor_is_unresolved(&pagination, &cursor) {
        return with_d1_bookmark(empty_timeline_response()?, &session);
    }
    let filter_matcher = match viewer {
        Some(viewer) => Some(load_account_filter_matcher(&db, viewer.id()).await?),
        None => None,
    };
    let (local_statuses, remote_statuses) = futures_util::try_join!(
        async {
            if include_local {
                list_local_public_statuses_by_tag(&db, &tag, &cursor, query_limit).await
            } else {
                Ok(Vec::new())
            }
        },
        async {
            if include_remote {
                list_remote_public_statuses_by_tag(&db, &tag, &cursor, query_limit).await
            } else {
                Ok(Vec::new())
            }
        },
    )?;
    let (local_accounts_by_id, mut media_by_status_id) =
        preload_local_timeline_rows(&db, &local_statuses).await?;
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
                &local_statuses.iter().collect::<Vec<_>>(),
            )
            .await?
        }
        None => HashSet::new(),
    };
    let mut candidates = Vec::new();

    if include_local {
        for status in local_statuses {
            let status_tags = extract_hashtags_from_text(&status.text);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            if !local_accounts_by_id.contains_key(&status.account_id) {
                continue;
            };
            if muted_local_status_ids.contains(&status.id) {
                continue;
            }
            let media = media_by_status_id.remove(&status.id).unwrap_or_default();
            if query.only_media.unwrap_or(false) && media.is_empty() {
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
        let remote_media_status_ids = remote_media_status_ids_for_filter(
            &db,
            query.only_media.unwrap_or(false),
            &remote_statuses,
        )
        .await?;
        for (status, actor) in remote_statuses {
            let status_tags = extract_hashtags_from_html(&status.content_html);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            if query.only_media.unwrap_or(false) && !remote_media_status_ids.contains(&status.id) {
                continue;
            }
            candidates.push(PublicTimelineCandidateEntry {
                timestamp: status.published_at.clone(),
                id: status.id.clone(),
                candidate: PublicTimelineCandidate::Remote { status, actor },
            });
        }
    }

    let candidates = select_public_timeline_candidates(candidates, limit);
    let entries = candidate_render::timeline_entries_from_candidates(
        &db,
        &config,
        viewer,
        filter_matcher.as_ref(),
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
