use crate::auth::{
    LocalApiAuthentication, authenticate_local_api_request, find_account_by_id,
    find_authenticated_local_account,
};
use crate::content_helpers::{extract_hashtags_from_html, extract_hashtags_from_text};
use crate::find_media_attachments_by_status_id;
use crate::find_remote_status_ids_with_media;
use crate::instance_identity::actor_url;
use crate::oauth_apps::{
    app_bearer_token_from_request, find_oauth_app_by_bearer_token,
    oauth_access_token_has_any_scope, oauth_app_has_any_scope,
};
use crate::runtime_config::load_config;
use crate::{
    HomeTimelineQuery, LinkTimelineQuery, PublicTimelineQuery, TagTimelineQuery,
    TimelinePaginationQuery, account_has_thread_mutes, build_local_status_response_with_preloads,
    build_local_status_response_with_quote_count_preloads,
    build_remote_status_response_with_preloaded_attachments,
    build_remote_status_response_with_preloads, build_status_card_value,
    build_timeline_link_header, canonicalize_link_timeline_url, derive_link_timeline_match_urls,
    enrich_card_with_remote_preview, find_remote_status_attachments_by_status_ids,
    include_local_source, include_remote_source, list_followed_tag_names,
    list_local_direct_timeline_statuses, list_local_home_timeline_statuses,
    list_local_public_statuses_by_link, list_local_public_statuses_by_tag,
    list_local_public_statuses_by_tags, list_local_public_timeline_statuses,
    list_remote_home_timeline_statuses, list_remote_public_statuses_by_link,
    list_remote_public_statuses_by_tag, list_remote_public_statuses_by_tags,
    list_remote_public_timeline_statuses, load_account_filter_matcher, load_in_reply_to_account_id,
    matches_tag_timeline_filters, normalize_hashtag, preload_local_status_viewer_state,
    preload_mastodon_poll_responses, preload_status_counts, preload_status_quote_counts,
    require_authenticated_local_account, resolve_timeline_cursor, strip_html_tags,
    timeline_fetch_limit, timeline_limit,
};
use crate::{is_local_status_thread_muted_by, is_muted_actor};
use cfwdon_core::TimelineAccessLevel;
use std::collections::{HashMap, HashSet};
use worker::D1Database;
use worker::{Error, Request, Response, Result, RouteContext};

pub(crate) enum TimelineRequestAccess {
    Viewer(crate::LocalAccount),
    ScopedApp,
    None,
    Invalid,
}

impl TimelineRequestAccess {
    fn viewer(&self) -> Option<&crate::LocalAccount> {
        match self {
            Self::Viewer(viewer) => Some(viewer),
            Self::ScopedApp | Self::None | Self::Invalid => None,
        }
    }

    fn is_authorized(&self) -> bool {
        matches!(self, Self::Viewer(_) | Self::ScopedApp)
    }
}

pub(crate) fn timeline_source_requires_authorization(level: TimelineAccessLevel) -> bool {
    !matches!(level, TimelineAccessLevel::Public)
}

pub(crate) fn timeline_request_requires_authorization(
    include_local: bool,
    include_remote: bool,
    local_access: TimelineAccessLevel,
    remote_access: TimelineAccessLevel,
) -> bool {
    (include_local && timeline_source_requires_authorization(local_access))
        || (include_remote && timeline_source_requires_authorization(remote_access))
}

pub(crate) async fn resolve_timeline_request_access(
    req: &Request,
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<TimelineRequestAccess> {
    if let Some(viewer) = find_authenticated_local_account(req, db, config).await? {
        return Ok(TimelineRequestAccess::Viewer(viewer));
    }

    let Some(token) = app_bearer_token_from_request(req)? else {
        return Ok(TimelineRequestAccess::None);
    };
    let Some(app) = find_oauth_app_by_bearer_token(db, &token).await? else {
        return Ok(TimelineRequestAccess::Invalid);
    };
    if oauth_app_has_any_scope(&app, &["read", "read:statuses"]) {
        Ok(TimelineRequestAccess::ScopedApp)
    } else {
        Ok(TimelineRequestAccess::Invalid)
    }
}

pub(crate) fn timeline_invalid_access_token_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "The access token is invalid",
    }))?
    .with_status(401))
}

pub(crate) fn timeline_outside_authorized_scopes_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is outside the authorized scopes",
    }))?
    .with_status(403))
}

type TimelineEntry = (String, String, serde_json::Value);

enum PublicTimelineCandidate {
    Local {
        status: crate::StatusRow,
        media: Vec<crate::MediaAttachmentRow>,
    },
    Remote {
        status: crate::RemoteStatusRow,
        actor: crate::RemoteActorRow,
    },
}

struct PublicTimelineCandidateEntry {
    timestamp: String,
    id: String,
    candidate: PublicTimelineCandidate,
}

type LocalTimelinePreload = (
    HashMap<String, crate::LocalAccount>,
    HashMap<String, Vec<crate::MediaAttachmentRow>>,
);

async fn preload_timeline_status_counts(
    db: &D1Database,
    local_statuses: &[crate::StatusRow],
    remote_statuses: &[(crate::RemoteStatusRow, crate::RemoteActorRow)],
) -> Result<crate::StatusCountsPreload> {
    let local_ids = local_statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();
    let remote_ids = remote_statuses
        .iter()
        .map(|(status, _)| status.id.clone())
        .collect::<Vec<_>>();
    preload_status_counts(db, &local_ids, &remote_ids).await
}

async fn preload_public_timeline_candidate_counts(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
) -> Result<crate::StatusCountsPreload> {
    let mut local_ids = Vec::new();
    let mut remote_ids = Vec::new();
    for entry in candidates {
        match &entry.candidate {
            PublicTimelineCandidate::Local { status, .. } => local_ids.push(status.id.clone()),
            PublicTimelineCandidate::Remote { status, .. } => remote_ids.push(status.id.clone()),
        }
    }

    preload_status_counts(db, &local_ids, &remote_ids).await
}

async fn preload_public_timeline_remote_attachments(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
) -> Result<HashMap<String, Vec<crate::RemoteStatusAttachmentRow>>> {
    let remote_ids = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Local { .. } => None,
            PublicTimelineCandidate::Remote { status, .. } => Some(status.id.clone()),
        })
        .collect::<Vec<_>>();

    find_remote_status_attachments_by_status_ids(db, &remote_ids).await
}

async fn preload_public_timeline_local_quote_counts(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    candidates: &[PublicTimelineCandidateEntry],
    accounts_by_id: &HashMap<String, crate::LocalAccount>,
) -> Result<crate::StatusQuoteCountsPreload> {
    let status_uris = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Remote { .. } => None,
            PublicTimelineCandidate::Local { status, .. } => accounts_by_id
                .get(&status.account_id)
                .map(|account| local_status_quote_count_uri(config, status, account)),
        })
        .collect::<Vec<_>>();

    preload_status_quote_counts(db, &status_uris).await
}

async fn preload_public_timeline_local_polls(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
    viewer: Option<&crate::LocalAccount>,
) -> Result<crate::MastodonPollResponsePreload> {
    let status_ids = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Remote { .. } => None,
            PublicTimelineCandidate::Local { status, .. } => Some(status.id.clone()),
        })
        .collect::<Vec<_>>();

    preload_mastodon_poll_responses(db, &status_ids, viewer).await
}

async fn preload_public_timeline_local_viewer_state(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
    viewer: Option<&crate::LocalAccount>,
) -> Result<crate::LocalStatusViewerStatePreload> {
    let Some(viewer) = viewer else {
        return Ok(crate::LocalStatusViewerStatePreload::default());
    };
    let statuses = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Remote { .. } => None,
            PublicTimelineCandidate::Local { status, .. } => Some(status),
        })
        .collect::<Vec<_>>();

    preload_local_status_viewer_state(db, &viewer.id, &statuses).await
}

fn local_status_quote_count_uri(
    config: &cfwdon_core::AppConfig,
    status: &crate::StatusRow,
    account: &crate::LocalAccount,
) -> String {
    status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, &account.username),
            status.id
        )
    })
}

async fn preload_local_timeline_rows(
    db: &D1Database,
    statuses: &[crate::StatusRow],
) -> Result<LocalTimelinePreload> {
    let account_ids = statuses
        .iter()
        .map(|status| status.account_id.clone())
        .collect::<Vec<_>>();
    let status_ids = statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();

    Ok((
        crate::find_accounts_by_ids(db, &account_ids).await?,
        crate::find_media_attachments_by_status_ids(db, &status_ids).await?,
    ))
}

async fn remote_media_status_ids_for_filter(
    db: &D1Database,
    only_media: bool,
    statuses: &[(crate::RemoteStatusRow, crate::RemoteActorRow)],
) -> Result<HashSet<String>> {
    if !only_media {
        return Ok(HashSet::new());
    }

    let status_ids = statuses
        .iter()
        .map(|(status, _)| status.id.clone())
        .collect::<Vec<_>>();
    find_remote_status_ids_with_media(db, &status_ids).await
}

fn select_public_timeline_candidates(
    mut candidates: Vec<PublicTimelineCandidateEntry>,
    limit: u32,
) -> Vec<PublicTimelineCandidateEntry> {
    candidates.sort_by(|left, right| {
        right
            .timestamp
            .cmp(&left.timestamp)
            .then_with(|| right.id.cmp(&left.id))
    });
    candidates.truncate(limit.saturating_add(1) as usize);
    candidates
}

fn timeline_cursor_requested(pagination: &TimelinePaginationQuery) -> bool {
    let has_max_id = pagination
        .max_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let has_min_id = pagination
        .min_id
        .as_deref()
        .or(pagination.since_id.as_deref())
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());

    has_max_id || has_min_id
}

fn timeline_cursor_is_unresolved(
    pagination: &TimelinePaginationQuery,
    cursor: &crate::ResolvedTimelineCursor,
) -> bool {
    let has_max_id = pagination
        .max_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let has_min_id = pagination
        .min_id
        .as_deref()
        .or(pagination.since_id.as_deref())
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());

    (has_max_id && cursor.max_timestamp.is_none()) || (has_min_id && cursor.min_timestamp.is_none())
}

fn empty_timeline_response() -> Result<Response> {
    Response::from_json(&Vec::<serde_json::Value>::new())
}

fn timeline_response_from_entries(
    req: &Request,
    limit: u32,
    entries: Vec<TimelineEntry>,
) -> Result<Response> {
    let (response, first_id, last_id) = timeline_page_response(entries, limit);
    let mut builder = Response::from_json(&response)?;
    if let Some(link) =
        build_timeline_link_header(req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}

fn timeline_page_response(
    mut entries: Vec<TimelineEntry>,
    limit: u32,
) -> (Vec<serde_json::Value>, Option<String>, Option<String>) {
    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let has_next_page = entries.len() > limit as usize;
    let page = entries.into_iter().take(limit as usize).collect::<Vec<_>>();
    let first_id = page
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = has_next_page
        .then(|| {
            page.last()
                .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()))
        })
        .flatten();
    let response = page
        .into_iter()
        .map(|(_, _, value)| value)
        .collect::<Vec<_>>();

    (response, first_id, last_id)
}

fn status_card_url_matches_targets(text: &str, targets: &HashSet<String>) -> bool {
    build_status_card_value(text)
        .and_then(|card| {
            card.get("url")
                .and_then(serde_json::Value::as_str)
                .and_then(canonicalize_link_timeline_url)
        })
        .map(|url| targets.contains(&url))
        .unwrap_or(false)
}

pub(crate) async fn home_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match authenticate_local_api_request(&req, &db, &config).await? {
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["read:statuses", "read"]) {
                return timeline_outside_authorized_scopes_response();
            }
            auth.account
        }
        LocalApiAuthentication::Access(viewer) => viewer,
        LocalApiAuthentication::AppToken
        | LocalApiAuthentication::InvalidBearer
        | LocalApiAuthentication::None => return timeline_invalid_access_token_response(),
    };
    let query: HomeTimelineQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    if timeline_cursor_is_unresolved(&query.pagination, &cursor) {
        return empty_timeline_response();
    }
    let filter_matcher = load_account_filter_matcher(&db, &viewer.id).await?;
    let mut entries = Vec::new();
    let mut seen_status_ids = HashSet::new();

    let local_home_statuses =
        list_local_home_timeline_statuses(&db, &viewer.id, &cursor, query_limit).await?;
    let remote_home_statuses =
        list_remote_home_timeline_statuses(&db, &viewer.id, &cursor, query_limit).await?;
    let counts_preload =
        preload_timeline_status_counts(&db, &local_home_statuses, &remote_home_statuses).await?;
    let (accounts_by_id, mut media_by_status_id) =
        preload_local_timeline_rows(&db, &local_home_statuses).await?;
    for status in local_home_statuses {
        if !seen_status_ids.insert(status.id.clone()) {
            continue;
        }
        let Some(account) = accounts_by_id.get(&status.account_id) else {
            continue;
        };
        if is_muted_actor(&db, &viewer.id, &actor_url(&config, &account.username)).await? {
            continue;
        }
        if is_local_status_thread_muted_by(&db, &viewer.id, &status).await? {
            continue;
        }
        let media = media_by_status_id.remove(&status.id).unwrap_or_default();
        entries.push((
            status.created_at.clone(),
            status.id.clone(),
            serde_json::to_value(
                build_local_status_response_with_preloads(
                    &db,
                    &config,
                    Some(&viewer),
                    &status,
                    account,
                    load_in_reply_to_account_id(&db, &status).await?,
                    media,
                    Some(&filter_matcher),
                    Some(&counts_preload),
                )
                .await?,
            )
            .unwrap_or(serde_json::Value::Null),
        ));
    }

    for (status, actor) in remote_home_statuses {
        if !seen_status_ids.insert(status.id.clone()) {
            continue;
        }
        if is_muted_actor(&db, &viewer.id, &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            status.id.clone(),
            serde_json::to_value(
                build_remote_status_response_with_preloads(
                    &db,
                    &config,
                    Some(&viewer),
                    &status,
                    &actor,
                    Some(&filter_matcher),
                    Some(&counts_preload),
                )
                .await?,
            )
            .unwrap_or(serde_json::Value::Null),
        ));
    }

    let followed_tags = list_followed_tag_names(&db, &viewer.id).await?;
    if !followed_tags.is_empty() {
        let local_tag_statuses =
            list_local_public_statuses_by_tags(&db, &followed_tags, &cursor, query_limit).await?;
        let remote_tag_statuses =
            list_remote_public_statuses_by_tags(&db, &followed_tags, &cursor, query_limit).await?;
        let counts_preload =
            preload_timeline_status_counts(&db, &local_tag_statuses, &remote_tag_statuses).await?;
        let (accounts_by_id, mut media_by_status_id) =
            preload_local_timeline_rows(&db, &local_tag_statuses).await?;
        for status in local_tag_statuses {
            if !seen_status_ids.insert(status.id.clone()) {
                continue;
            }
            let Some(account) = accounts_by_id.get(&status.account_id) else {
                continue;
            };
            if is_muted_actor(&db, &viewer.id, &actor_url(&config, &account.username)).await? {
                continue;
            }
            if is_local_status_thread_muted_by(&db, &viewer.id, &status).await? {
                continue;
            }
            let media = media_by_status_id.remove(&status.id).unwrap_or_default();
            entries.push((
                status.created_at.clone(),
                status.id.clone(),
                serde_json::to_value(
                    build_local_status_response_with_preloads(
                        &db,
                        &config,
                        Some(&viewer),
                        &status,
                        account,
                        load_in_reply_to_account_id(&db, &status).await?,
                        media,
                        Some(&filter_matcher),
                        Some(&counts_preload),
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null),
            ));
        }

        for (status, actor) in remote_tag_statuses {
            if !seen_status_ids.insert(status.id.clone()) {
                continue;
            }
            if is_muted_actor(&db, &viewer.id, &actor.actor_uri).await? {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                status.id.clone(),
                serde_json::to_value(
                    build_remote_status_response_with_preloads(
                        &db,
                        &config,
                        Some(&viewer),
                        &status,
                        &actor,
                        Some(&filter_matcher),
                        Some(&counts_preload),
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null),
            ));
        }
    }

    timeline_response_from_entries(&req, limit, entries)
}

pub(crate) async fn public_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: PublicTimelineQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let include_local = include_local_source(query.local, query.remote);
    let include_remote = include_remote_source(query.local, query.remote);
    let db = ctx.d1(&config.database_binding)?;
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
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    if timeline_cursor_is_unresolved(&query.pagination, &cursor) {
        return empty_timeline_response();
    }
    let filter_matcher = match viewer {
        Some(viewer) => Some(load_account_filter_matcher(&db, &viewer.id).await?),
        None => None,
    };
    let local_statuses = if include_local {
        list_local_public_timeline_statuses(&db, &cursor, query_limit).await?
    } else {
        Vec::new()
    };
    let remote_statuses = if include_remote {
        list_remote_public_timeline_statuses(&db, &cursor, query_limit).await?
    } else {
        Vec::new()
    };
    let only_media = query.only_media.unwrap_or(false);
    let mut candidates = Vec::new();
    let mut local_accounts_by_id = HashMap::new();
    let viewer_has_thread_mutes = match viewer {
        Some(viewer) => account_has_thread_mutes(&db, &viewer.id).await?,
        None => false,
    };

    if include_local {
        let (accounts_by_id, mut media_by_status_id) =
            preload_local_timeline_rows(&db, &local_statuses).await?;
        local_accounts_by_id = accounts_by_id;
        for status in local_statuses {
            if !local_accounts_by_id.contains_key(&status.account_id) {
                continue;
            }
            if viewer_has_thread_mutes
                && let Some(viewer) = viewer
                && is_local_status_thread_muted_by(&db, &viewer.id, &status).await?
            {
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
            remote_media_status_ids_for_filter(&db, only_media, &remote_statuses).await?;
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

    let candidates = select_public_timeline_candidates(candidates, limit);
    let counts_preload = preload_public_timeline_candidate_counts(&db, &candidates).await?;
    let local_quote_counts_preload = preload_public_timeline_local_quote_counts(
        &db,
        &config,
        &candidates,
        &local_accounts_by_id,
    )
    .await?;
    let local_poll_preload = preload_public_timeline_local_polls(&db, &candidates, viewer).await?;
    let local_viewer_state_preload =
        preload_public_timeline_local_viewer_state(&db, &candidates, viewer).await?;
    let mut remote_attachments_by_status_id =
        preload_public_timeline_remote_attachments(&db, &candidates).await?;
    let mut entries = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        match candidate.candidate {
            PublicTimelineCandidate::Local { status, media } => {
                let Some(account) = local_accounts_by_id.get(&status.account_id) else {
                    continue;
                };
                entries.push((
                    candidate.timestamp,
                    candidate.id,
                    serde_json::to_value(
                        build_local_status_response_with_quote_count_preloads(
                            &db,
                            &config,
                            viewer,
                            &status,
                            account,
                            None,
                            media,
                            filter_matcher.as_ref(),
                            Some(&counts_preload),
                            Some(&local_quote_counts_preload),
                            Some(&local_poll_preload),
                            Some(&local_viewer_state_preload),
                        )
                        .await?,
                    )
                    .unwrap_or(serde_json::Value::Null),
                ));
            }
            PublicTimelineCandidate::Remote { status, actor } => {
                let remote_attachments = remote_attachments_by_status_id
                    .remove(&status.id)
                    .unwrap_or_default();
                entries.push((
                    candidate.timestamp,
                    candidate.id,
                    serde_json::to_value(
                        build_remote_status_response_with_preloaded_attachments(
                            &db,
                            &config,
                            viewer,
                            &status,
                            &actor,
                            filter_matcher.as_ref(),
                            Some(&counts_preload),
                            remote_attachments,
                        )
                        .await?,
                    )
                    .unwrap_or(serde_json::Value::Null),
                ));
            }
        }
    }

    timeline_response_from_entries(&req, limit, entries)
}

pub(crate) async fn tag_timeline_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("hashtag")
        .map(|value| normalize_hashtag(&value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing hashtag route parameter".to_owned()))?;
    let query: TagTimelineQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let include_local = include_local_source(query.local, query.remote);
    let include_remote = include_remote_source(query.local, query.remote);
    let db = ctx.d1(&config.database_binding)?;
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
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    if timeline_cursor_is_unresolved(&query.pagination, &cursor) {
        return empty_timeline_response();
    }
    let filter_matcher = match viewer {
        Some(viewer) => Some(load_account_filter_matcher(&db, &viewer.id).await?),
        None => None,
    };
    let mut entries = Vec::new();
    let local_statuses = if include_local {
        list_local_public_statuses_by_tag(&db, &tag, &cursor, query_limit).await?
    } else {
        Vec::new()
    };
    let remote_statuses = if include_remote {
        list_remote_public_statuses_by_tag(&db, &tag, &cursor, query_limit).await?
    } else {
        Vec::new()
    };
    let counts_preload =
        preload_timeline_status_counts(&db, &local_statuses, &remote_statuses).await?;

    if include_local {
        let (accounts_by_id, mut media_by_status_id) =
            preload_local_timeline_rows(&db, &local_statuses).await?;
        for status in local_statuses {
            let status_tags = extract_hashtags_from_text(&status._text_content);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            let Some(account) = accounts_by_id.get(&status.account_id) else {
                continue;
            };
            if let Some(viewer) = viewer
                && is_local_status_thread_muted_by(&db, &viewer.id, &status).await?
            {
                continue;
            }
            let media = media_by_status_id.remove(&status.id).unwrap_or_default();
            if query.only_media.unwrap_or(false) && media.is_empty() {
                continue;
            }
            entries.push((
                status.created_at.clone(),
                status.id.clone(),
                serde_json::to_value(
                    build_local_status_response_with_preloads(
                        &db,
                        &config,
                        viewer,
                        &status,
                        account,
                        load_in_reply_to_account_id(&db, &status).await?,
                        media,
                        filter_matcher.as_ref(),
                        Some(&counts_preload),
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null),
            ));
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
            entries.push((
                status.published_at.clone(),
                status.id.clone(),
                serde_json::to_value(
                    build_remote_status_response_with_preloads(
                        &db,
                        &config,
                        viewer,
                        &status,
                        &actor,
                        filter_matcher.as_ref(),
                        Some(&counts_preload),
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null),
            ));
        }
    }

    timeline_response_from_entries(&req, limit, entries)
}

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
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let db = ctx.d1(&config.database_binding)?;
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
        .filter_map(|url| canonicalize_link_timeline_url(&url))
        .collect::<HashSet<_>>();
    let viewer = access.viewer();
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;
    if timeline_cursor_is_unresolved(&query.pagination, &cursor) {
        return empty_timeline_response();
    }
    let filter_matcher = match viewer {
        Some(viewer) => Some(load_account_filter_matcher(&db, &viewer.id).await?),
        None => None,
    };
    let mut entries = Vec::new();

    let local_link_statuses =
        list_local_public_statuses_by_link(&db, &target_urls, &cursor, query_limit).await?;
    let remote_link_statuses =
        list_remote_public_statuses_by_link(&db, &target_urls, &cursor, query_limit).await?;
    let counts_preload =
        preload_timeline_status_counts(&db, &local_link_statuses, &remote_link_statuses).await?;
    let (accounts_by_id, mut media_by_status_id) =
        preload_local_timeline_rows(&db, &local_link_statuses).await?;
    for status in local_link_statuses {
        if !status_card_url_matches_targets(&status._text_content, &target_url_set) {
            continue;
        }
        let Some(account) = accounts_by_id.get(&status.account_id) else {
            continue;
        };
        if let Some(viewer) = viewer
            && is_local_status_thread_muted_by(&db, &viewer.id, &status).await?
        {
            continue;
        }
        let media = media_by_status_id.remove(&status.id).unwrap_or_default();
        let mut value = serde_json::to_value(
            build_local_status_response_with_preloads(
                &db,
                &config,
                viewer,
                &status,
                account,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
                filter_matcher.as_ref(),
                Some(&counts_preload),
            )
            .await?,
        )
        .unwrap_or(serde_json::Value::Null);
        if let Some(card) = value.get_mut("card") {
            let _ = enrich_card_with_remote_preview(card).await;
        }
        entries.push((status.created_at.clone(), status.id.clone(), value));
    }

    for (status, actor) in remote_link_statuses {
        if !status_card_url_matches_targets(&strip_html_tags(&status.content_html), &target_url_set)
        {
            continue;
        }
        let mut value = serde_json::to_value(
            build_remote_status_response_with_preloads(
                &db,
                &config,
                viewer,
                &status,
                &actor,
                filter_matcher.as_ref(),
                Some(&counts_preload),
            )
            .await?,
        )
        .unwrap_or(serde_json::Value::Null);
        if let Some(card) = value.get_mut("card") {
            let _ = enrich_card_with_remote_preview(card).await;
        }
        entries.push((status.published_at.clone(), status.id.clone(), value));
    }

    if entries.is_empty() && !timeline_cursor_requested(&query.pagination) {
        return Response::error("Record not found", 404);
    }
    timeline_response_from_entries(&req, limit, entries)
}

#[cfg(test)]
mod tests {
    use super::{
        PublicTimelineCandidate, PublicTimelineCandidateEntry, select_public_timeline_candidates,
        status_card_url_matches_targets, timeline_page_response,
        timeline_request_requires_authorization, timeline_source_requires_authorization,
    };
    use cfwdon_core::TimelineAccessLevel;
    use std::collections::HashSet;

    fn timeline_test_entry(created_at: &str, id: &str) -> super::TimelineEntry {
        (
            created_at.to_owned(),
            id.to_owned(),
            serde_json::json!({ "id": id }),
        )
    }

    fn public_timeline_test_candidate(
        created_at: &str,
        id: &str,
    ) -> super::PublicTimelineCandidateEntry {
        PublicTimelineCandidateEntry {
            timestamp: created_at.to_owned(),
            id: id.to_owned(),
            candidate: PublicTimelineCandidate::Local {
                status: crate::StatusRow {
                    id: id.to_owned(),
                    account_id: "account".to_owned(),
                    ap_id: None,
                    in_reply_to_id: None,
                    boost_of_uri: None,
                    quote_of_uri: None,
                    content_html: String::new(),
                    _text_content: String::new(),
                    spoiler_text: String::new(),
                    visibility: "public".to_owned(),
                    sensitive: 0,
                    language: None,
                    quote_approval_policy: None,
                    quote_state: "accepted".to_owned(),
                    application_id: None,
                    created_at: created_at.to_owned(),
                    updated_at: Some(created_at.to_owned()),
                },
                media: Vec::new(),
            },
        }
    }

    #[test]
    fn timeline_page_response_uses_returned_page_for_cursor_bounds() {
        let entries = vec![
            timeline_test_entry("2026-05-09T00:00:01Z", "first"),
            timeline_test_entry("2026-05-09T00:00:00Z", "second"),
            timeline_test_entry("2026-05-08T23:59:59Z", "not-returned"),
        ];

        let (page, first_id, last_id) = timeline_page_response(entries, 2);

        assert_eq!(
            page.iter()
                .filter_map(|value| value.get("id").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert_eq!(first_id.as_deref(), Some("first"));
        assert_eq!(last_id.as_deref(), Some("second"));
    }

    #[test]
    fn timeline_page_response_sorts_timestamp_and_id_descending() {
        let entries = vec![
            timeline_test_entry("2026-05-09T00:00:00Z", "b"),
            timeline_test_entry("2026-05-09T00:00:00Z", "c"),
            timeline_test_entry("2026-05-09T00:00:00Z", "a"),
        ];

        let (page, first_id, last_id) = timeline_page_response(entries, 2);

        assert_eq!(
            page.iter()
                .filter_map(|value| value.get("id").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>(),
            vec!["c", "b"]
        );
        assert_eq!(first_id.as_deref(), Some("c"));
        assert_eq!(last_id.as_deref(), Some("b"));
    }

    #[test]
    fn timeline_page_response_omits_next_cursor_when_page_is_exhausted() {
        let entries = vec![
            timeline_test_entry("2026-05-09T00:00:01Z", "first"),
            timeline_test_entry("2026-05-09T00:00:00Z", "second"),
        ];

        let (_page, first_id, last_id) = timeline_page_response(entries, 20);

        assert_eq!(first_id.as_deref(), Some("first"));
        assert_eq!(last_id, None);
    }

    #[test]
    fn select_public_timeline_candidates_keeps_only_page_and_next_cursor_candidate() {
        let candidates = vec![
            public_timeline_test_candidate("2026-05-09T00:00:01Z", "first"),
            public_timeline_test_candidate("2026-05-09T00:00:03Z", "third"),
            public_timeline_test_candidate("2026-05-09T00:00:02Z", "second"),
            public_timeline_test_candidate("2026-05-09T00:00:00Z", "not-hydrated"),
        ];

        let selected = select_public_timeline_candidates(candidates, 2);

        assert_eq!(
            selected
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["third", "second", "first"]
        );
    }

    #[test]
    fn timeline_source_requires_authorization_for_non_public_levels() {
        assert!(!timeline_source_requires_authorization(
            TimelineAccessLevel::Public
        ));
        assert!(timeline_source_requires_authorization(
            TimelineAccessLevel::Authenticated
        ));
        assert!(timeline_source_requires_authorization(
            TimelineAccessLevel::Disabled
        ));
    }

    #[test]
    fn timeline_request_requires_authorization_only_for_requested_sources() {
        assert!(!timeline_request_requires_authorization(
            true,
            false,
            TimelineAccessLevel::Public,
            TimelineAccessLevel::Authenticated,
        ));
        assert!(timeline_request_requires_authorization(
            true,
            true,
            TimelineAccessLevel::Public,
            TimelineAccessLevel::Authenticated,
        ));
        assert!(timeline_request_requires_authorization(
            false,
            true,
            TimelineAccessLevel::Public,
            TimelineAccessLevel::Disabled,
        ));
    }

    #[test]
    fn status_card_url_matches_targets_uses_primary_card_url() {
        let targets = ["https://example.com/articles/rust".to_owned()]
            .into_iter()
            .collect::<HashSet<_>>();

        assert!(status_card_url_matches_targets(
            "see https://example.com/articles/rust and https://example.com/other",
            &targets,
        ));
        assert!(!status_card_url_matches_targets(
            "see https://example.com/other and https://example.com/articles/rust",
            &targets,
        ));
    }
}

pub(crate) async fn direct_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: TimelinePaginationQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query);
    let query_limit = timeline_fetch_limit(limit);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let cursor = resolve_timeline_cursor(&db, &query).await?;
    let filter_matcher = load_account_filter_matcher(&db, &viewer.id).await?;
    let mut entries = Vec::new();
    let direct_statuses =
        list_local_direct_timeline_statuses(&db, &viewer.id, &cursor, query_limit).await?;
    let direct_counts_preload = preload_timeline_status_counts(&db, &direct_statuses, &[]).await?;

    for status in direct_statuses {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        if is_muted_actor(&db, &viewer.id, &actor_url(&config, &account.username)).await? {
            continue;
        }
        if is_local_status_thread_muted_by(&db, &viewer.id, &status).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            status.id.clone(),
            build_local_status_response_with_preloads(
                &db,
                &config,
                Some(&viewer),
                &status,
                &account,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
                Some(&filter_matcher),
                Some(&direct_counts_preload),
            )
            .await?,
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let first_id = entries
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = entries
        .last()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let response = entries
        .into_iter()
        .map(|(_, _, value)| value)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let mut builder = Response::from_json(&response)?;
    if let Some(link) =
        build_timeline_link_header(&req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}
