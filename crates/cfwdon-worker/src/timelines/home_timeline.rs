use super::{
    candidate_render, empty_timeline_response, local_status_actor_uri,
    muted_local_timeline_status_ids, preload_local_timeline_rows_from_status_refs,
    resolve_timeline_cursor, select_public_timeline_candidates, timeline_cursor_is_unresolved,
    timeline_fetch_limit, timeline_invalid_access_token_response,
    timeline_outside_authorized_scopes_response, timeline_limit, timeline_response_from_entries,
    HomeTimelineQuery, PublicTimelineCandidate, PublicTimelineCandidateEntry,
};
use crate::auth::{authenticate_local_api_request, LocalApiAuthentication};
use crate::oauth_apps::oauth_access_token_has_any_scope;
use crate::relationship::list_active_muted_actor_uris_for_account;
use crate::runtime_config::load_config;
use crate::{
    find_remote_statuses_with_actors_by_ids, find_statuses_by_ids,
    list_home_timeline_candidate_ids, load_account_filter_matcher, open_bound_request_session,
    with_d1_bookmark, HOME_TIMELINE_CANDIDATE_SOURCE_LOCAL, HOME_TIMELINE_CANDIDATE_SOURCE_REMOTE,
};
use std::collections::{HashMap, HashSet};
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn home_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (session, db) = open_bound_request_session(&ctx, &config, &req)?;
    let query: HomeTimelineQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
    let query_limit = timeline_fetch_limit(limit);
    let (auth, cursor) = futures_util::try_join!(
        authenticate_local_api_request(&req, &db, &config),
        resolve_timeline_cursor(&db, &pagination),
    )?;
    let viewer = match auth {
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["read:statuses", "read"]) {
                return timeline_outside_authorized_scopes_response();
            }
            auth.account
        }
        LocalApiAuthentication::Auth0(viewer) => viewer,
        LocalApiAuthentication::AppToken
        | LocalApiAuthentication::InvalidBearer
        | LocalApiAuthentication::None => return timeline_invalid_access_token_response(),
    };
    if timeline_cursor_is_unresolved(&pagination, &cursor) {
        return with_d1_bookmark(empty_timeline_response()?, &session);
    }
    let (filter_matcher, viewer_has_thread_mutes, include_followed_tags, muted_actor_uris) = {
        let caps = crate::load_account_capabilities(&db, viewer.id()).await?;
        let (filter_matcher, muted_actor_uris) = futures_util::try_join!(
            async {
                if caps.has_filters {
                    load_account_filter_matcher(&db, viewer.id()).await
                } else {
                    Ok(crate::AccountFilterMatcher::default())
                }
            },
            list_active_muted_actor_uris_for_account(&db, viewer.id()),
        )?;
        (
            filter_matcher,
            caps.has_thread_mutes,
            caps.has_followed_tags,
            muted_actor_uris,
        )
    };
    let candidate_rows = list_home_timeline_candidate_ids(
        &db,
        viewer.id(),
        &cursor,
        query_limit,
        include_followed_tags,
    )
    .await?;

    let mut local_candidate_ids = Vec::new();
    let mut remote_candidate_ids = Vec::new();
    for row in &candidate_rows {
        match row.source.as_str() {
            HOME_TIMELINE_CANDIDATE_SOURCE_LOCAL => {
                local_candidate_ids.push(row.status_id.clone());
            }
            HOME_TIMELINE_CANDIDATE_SOURCE_REMOTE => {
                remote_candidate_ids.push(row.status_id.clone());
            }
            _ => {}
        }
    }

    let (local_statuses, remote_statuses) = futures_util::try_join!(
        find_statuses_by_ids(&db, &local_candidate_ids),
        find_remote_statuses_with_actors_by_ids(&db, &remote_candidate_ids),
    )?;
    let mut local_statuses_by_id = local_statuses
        .into_iter()
        .map(|status| (status.id.clone(), status))
        .collect::<HashMap<_, _>>();
    let mut remote_statuses_by_id = remote_statuses
        .into_iter()
        .map(|(status, actor)| (status.id.clone(), (status, actor)))
        .collect::<HashMap<_, _>>();
    // Thread mutes key off the candidate statuses, not their authors, so they no
    // longer have to wait for the account lookup.
    let ((local_accounts_by_id, mut media_by_status_id), muted_local_status_ids) = {
        let local_status_refs = local_statuses_by_id.values().collect::<Vec<_>>();
        futures_util::try_join!(
            preload_local_timeline_rows_from_status_refs(&db, &local_status_refs),
            muted_local_timeline_status_ids(
                &db,
                viewer.id(),
                viewer_has_thread_mutes,
                &local_status_refs,
            ),
        )?
    };
    let mut candidates = Vec::new();
    let mut seen_status_ids = HashSet::new();

    for row in candidate_rows {
        if !seen_status_ids.insert(row.status_id.clone()) {
            continue;
        }
        match row.source.as_str() {
            HOME_TIMELINE_CANDIDATE_SOURCE_LOCAL => {
                let Some(status) = local_statuses_by_id.remove(&row.status_id) else {
                    continue;
                };
                let Some(actor_uri) =
                    local_status_actor_uri(&config, &local_accounts_by_id, &status)
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
                    timestamp: row.timestamp,
                    id: status.id.clone(),
                    candidate: PublicTimelineCandidate::Local { status, media },
                });
            }
            HOME_TIMELINE_CANDIDATE_SOURCE_REMOTE => {
                let Some((status, actor)) = remote_statuses_by_id.remove(&row.status_id) else {
                    continue;
                };
                if muted_actor_uris.contains(&actor.actor_uri) {
                    continue;
                }
                candidates.push(PublicTimelineCandidateEntry {
                    timestamp: row.timestamp,
                    id: status.id.clone(),
                    candidate: PublicTimelineCandidate::Remote { status, actor },
                });
            }
            _ => {}
        }
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
