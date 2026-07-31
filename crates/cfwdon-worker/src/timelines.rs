#[allow(unused_imports)]
pub(crate) use crate::*;

mod request_parsing;
mod search;
pub(crate) use request_parsing::*;
#[allow(unused_imports)]
pub(crate) use search::*;

pub(crate) use self::request_parsing::{
    HomeTimelineQuery, LinkTimelineQuery, PublicTimelineQuery, TagTimelineQuery,
    TimelinePaginationQuery, build_timeline_link_header, canonicalize_link_timeline_url,
    derive_link_timeline_match_urls, include_local_source, include_remote_source,
    matches_tag_timeline_filters, resolve_timeline_cursor, timeline_fetch_limit, timeline_limit,
};
use crate::actor_url;
use crate::auth::{
    LocalApiAuthentication, authenticate_local_api_request, find_authenticated_local_account,
};
use crate::content_helpers::{extract_hashtags_from_html, extract_hashtags_from_text};
use crate::find_remote_status_ids_with_media;
use crate::local_status_ids_thread_muted_by;
use crate::oauth_apps::{
    app_bearer_token_from_request, find_oauth_app_by_bearer_token,
    oauth_access_token_has_any_scope, oauth_app_has_any_scope,
};
use crate::runtime_config::load_config;
use crate::{
    HOME_TIMELINE_CANDIDATE_SOURCE_LOCAL, HOME_TIMELINE_CANDIDATE_SOURCE_REMOTE,
    account_has_followed_tags, account_has_thread_mutes,
    build_local_status_response_with_timeline_preloads,
    build_remote_status_response_with_timeline_preloads, build_status_card_value,
    enrich_card_with_remote_preview, find_remote_status_attachments_by_status_ids,
    find_remote_statuses_with_actors_by_ids, find_statuses_by_ids, list_active_muted_actor_uris,
    list_home_timeline_candidate_ids, list_local_direct_timeline_statuses,
    list_local_public_statuses_by_link, list_local_public_statuses_by_tag,
    list_local_public_timeline_statuses, list_remote_direct_statuses_mentioning_viewer,
    list_remote_public_statuses_by_link, list_remote_public_statuses_by_tag,
    list_remote_public_timeline_statuses, load_account_filter_matcher, normalize_hashtag,
    preload_local_status_viewer_state, preload_mastodon_poll_responses,
    preload_remote_mastodon_poll_responses, preload_remote_status_edit_updated_at,
    preload_remote_status_viewer_state, preload_status_applications, preload_status_counts,
    preload_status_quote_counts, require_authenticated_local_account, strip_html_tags,
};
use cfwdon_core::TimelineAccessLevel;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use worker::D1Database;
use worker::d1::D1Type;
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

#[derive(Debug, Deserialize)]
struct ReplyAccountIdRow {
    id: String,
    account_id: String,
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

async fn preload_public_timeline_quote_counts(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    candidates: &[PublicTimelineCandidateEntry],
    accounts_by_id: &HashMap<String, crate::LocalAccount>,
) -> Result<crate::StatusQuoteCountsPreload> {
    let status_uris = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Remote { status, .. } => Some(status.object_uri.clone()),
            PublicTimelineCandidate::Local { status, .. } => accounts_by_id
                .get(&status.account_id)
                .map(|account| local_status_quote_count_uri(config, status, account)),
        })
        .collect::<Vec<_>>();

    preload_status_quote_counts(db, &status_uris).await
}

async fn muted_local_timeline_status_ids(
    db: &D1Database,
    viewer_account_id: &str,
    viewer_has_thread_mutes: bool,
    statuses: &[&crate::StatusRow],
) -> Result<HashSet<String>> {
    if !viewer_has_thread_mutes || statuses.is_empty() {
        return Ok(HashSet::new());
    }
    local_status_ids_thread_muted_by(db, viewer_account_id, statuses).await
}

async fn preload_public_timeline_remote_viewer_state(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
    viewer: Option<&crate::LocalAccount>,
) -> Result<crate::RemoteStatusViewerStatePreload> {
    let Some(viewer) = viewer else {
        return Ok(crate::RemoteStatusViewerStatePreload::default());
    };
    let statuses = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Local { .. } => None,
            PublicTimelineCandidate::Remote { status, actor } => Some((status, actor)),
        })
        .collect::<Vec<_>>();

    preload_remote_status_viewer_state(db, viewer.id(), &statuses).await
}

async fn preload_public_timeline_remote_polls(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
    viewer: Option<&crate::LocalAccount>,
) -> Result<crate::RemoteMastodonPollResponsePreload> {
    let status_ids = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Local { .. } => None,
            PublicTimelineCandidate::Remote { status, .. } => Some(status.id.clone()),
        })
        .collect::<Vec<_>>();

    preload_remote_mastodon_poll_responses(db, &status_ids, viewer).await
}

async fn preload_public_timeline_remote_edits(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
) -> Result<crate::RemoteStatusEditUpdatedAtPreload> {
    let status_ids = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Local { .. } => None,
            PublicTimelineCandidate::Remote { status, .. } => Some(status.id.clone()),
        })
        .collect::<Vec<_>>();

    preload_remote_status_edit_updated_at(db, &status_ids).await
}

async fn preload_public_timeline_remote_federated_emojis(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
) -> Result<crate::RemoteStatusFederatedEmojisPreload> {
    let status_ids = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Local { .. } => None,
            PublicTimelineCandidate::Remote { status, .. } => Some(status.id.clone()),
        })
        .collect::<Vec<_>>();

    preload_remote_status_federated_emojis(db, &status_ids).await
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
    known_has_thread_mutes: Option<bool>,
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

    preload_local_status_viewer_state(db, viewer.id(), &statuses, known_has_thread_mutes).await
}

async fn preload_timeline_candidate_reply_account_ids(
    db: &D1Database,
    candidates: &[PublicTimelineCandidateEntry],
) -> Result<HashMap<String, String>> {
    let mut seen = HashSet::new();
    let reply_ids = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Remote { .. } => None,
            PublicTimelineCandidate::Local { status, .. } => status.in_reply_to_id.as_ref(),
        })
        .filter(|id| seen.insert(id.as_str()))
        .collect::<Vec<_>>();
    if reply_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let reply_ids_json = crate::json_string_array(&reply_ids);
    let sql = format!(
        "SELECT id, account_id
         FROM statuses
         WHERE id {}",
        crate::sql_in_json_each(1)
    );
    let binding = D1Type::Text(reply_ids_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;
    let reply_accounts_by_status_id = result
        .results::<ReplyAccountIdRow>()?
        .into_iter()
        .map(|row| (row.id, row.account_id))
        .collect::<HashMap<_, _>>();

    Ok(candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Remote { .. } => None,
            PublicTimelineCandidate::Local { status, .. } => status
                .in_reply_to_id
                .as_ref()
                .and_then(|reply_id| reply_accounts_by_status_id.get(reply_id))
                .map(|account_id| (status.id.clone(), account_id.clone())),
        })
        .collect())
}

fn local_status_quote_count_uri(
    config: &cfwdon_core::AppConfig,
    status: &crate::StatusRow,
    account: &crate::LocalAccount,
) -> String {
    status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, account.username()),
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

    futures_util::try_join!(
        crate::find_accounts_by_ids(db, &account_ids),
        crate::find_media_attachments_by_status_ids(db, &status_ids),
    )
}

async fn preload_local_timeline_rows_from_status_refs(
    db: &D1Database,
    statuses: &[&crate::StatusRow],
) -> Result<LocalTimelinePreload> {
    let account_ids = statuses
        .iter()
        .map(|status| status.account_id.clone())
        .collect::<Vec<_>>();
    let status_ids = statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();

    futures_util::try_join!(
        crate::find_accounts_by_ids(db, &account_ids),
        crate::find_media_attachments_by_status_ids(db, &status_ids),
    )
}

fn local_status_actor_uri(
    config: &cfwdon_core::AppConfig,
    accounts_by_id: &HashMap<String, crate::LocalAccount>,
    status: &crate::StatusRow,
) -> Option<String> {
    accounts_by_id
        .get(&status.account_id)
        .map(|account| actor_url(config, account.username()))
}

async fn preload_muted_timeline_actor_uris(
    db: &D1Database,
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

async fn timeline_entries_from_candidates(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    filter_matcher: Option<&crate::AccountFilterMatcher>,
    local_accounts_by_id: &HashMap<String, crate::LocalAccount>,
    candidates: Vec<PublicTimelineCandidateEntry>,
    enrich_cards: bool,
    known_viewer_has_thread_mutes: Option<bool>,
) -> Result<Vec<TimelineEntry>> {
    let mut mention_texts = Vec::with_capacity(candidates.len());
    let mut remote_text_owned = Vec::new();
    for candidate in &candidates {
        match &candidate.candidate {
            PublicTimelineCandidate::Local { status, .. } => {
                mention_texts.push(status.text.as_str());
            }
            PublicTimelineCandidate::Remote { status, .. } => {
                remote_text_owned.push(strip_html_tags(&status.content_html));
            }
        }
    }
    for text in &remote_text_owned {
        mention_texts.push(text.as_str());
    }

    let (
        counts_preload,
        quote_counts_preload,
        local_poll_preload,
        local_viewer_state_preload,
        remote_viewer_state_preload,
        remote_poll_preload,
        remote_edit_updated_at_preload,
        remote_federated_emojis_preload,
        in_reply_to_account_ids,
        application_preload,
        mut remote_attachments_by_status_id,
        mention_preload,
        emoji_resolved_config,
    ) = futures_util::try_join!(
        preload_public_timeline_candidate_counts(db, &candidates),
        preload_public_timeline_quote_counts(db, config, &candidates, local_accounts_by_id),
        preload_public_timeline_local_polls(db, &candidates, viewer),
        preload_public_timeline_local_viewer_state(
            db,
            &candidates,
            viewer,
            known_viewer_has_thread_mutes,
        ),
        preload_public_timeline_remote_viewer_state(db, &candidates, viewer),
        preload_public_timeline_remote_polls(db, &candidates, viewer),
        preload_public_timeline_remote_edits(db, &candidates),
        preload_public_timeline_remote_federated_emojis(db, &candidates),
        preload_timeline_candidate_reply_account_ids(db, &candidates),
        preload_public_timeline_status_applications(db, config, &candidates),
        preload_public_timeline_remote_attachments(db, &candidates),
        crate::preload_mention_accounts_from_texts(db, config, &mention_texts),
        crate::config_with_resolved_custom_emojis(db, config),
    )?;
    let mut entries = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        match candidate.candidate {
            PublicTimelineCandidate::Local { status, media } => {
                let Some(account) = local_accounts_by_id.get(&status.account_id) else {
                    continue;
                };
                let mut value = serde_json::to_value(
                    build_local_status_response_with_timeline_preloads(
                        db,
                        config,
                        Some(&emoji_resolved_config),
                        viewer,
                        &status,
                        account,
                        in_reply_to_account_ids.get(&status.id).cloned(),
                        media,
                        filter_matcher,
                        Some(&counts_preload),
                        Some(&quote_counts_preload),
                        Some(&local_poll_preload),
                        Some(&local_viewer_state_preload),
                        Some(&application_preload),
                        Some(&mention_preload),
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null);
                if enrich_cards && let Some(card) = value.get_mut("card") {
                    let _ = enrich_card_with_remote_preview(card).await;
                }
                entries.push((candidate.timestamp, candidate.id, value));
            }
            PublicTimelineCandidate::Remote { status, actor } => {
                let remote_attachments = remote_attachments_by_status_id
                    .remove(&status.id)
                    .unwrap_or_default();
                let mut value = serde_json::to_value(
                    build_remote_status_response_with_timeline_preloads(
                        db,
                        config,
                        viewer,
                        &status,
                        &actor,
                        filter_matcher,
                        Some(&counts_preload),
                        Some(&quote_counts_preload),
                        Some(&remote_viewer_state_preload),
                        Some(&remote_poll_preload),
                        Some(&remote_edit_updated_at_preload),
                        Some(&remote_federated_emojis_preload),
                        remote_attachments,
                        Some(&mention_preload),
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null);
                if enrich_cards && let Some(card) = value.get_mut("card") {
                    let _ = enrich_card_with_remote_preview(card).await;
                }
                entries.push((candidate.timestamp, candidate.id, value));
            }
        }
    }

    Ok(entries)
}

async fn preload_public_timeline_status_applications(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    candidates: &[PublicTimelineCandidateEntry],
) -> Result<crate::StatusApplicationPreload> {
    let statuses = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Local { status, .. } => Some(status),
            PublicTimelineCandidate::Remote { .. } => None,
        })
        .collect::<Vec<_>>();

    preload_status_applications(db, config, &statuses).await
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
        return empty_timeline_response();
    }
    let (filter_matcher, viewer_has_thread_mutes, include_followed_tags) = futures_util::try_join!(
        load_account_filter_matcher(&db, viewer.id()),
        account_has_thread_mutes(&db, viewer.id()),
        account_has_followed_tags(&db, viewer.id()),
    )?;
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
    let (local_accounts_by_id, mut media_by_status_id, muted_actor_uris) = {
        let local_status_refs = local_statuses_by_id.values().collect::<Vec<_>>();
        let remote_status_refs = remote_statuses_by_id.values().collect::<Vec<_>>();
        let (local_accounts_by_id, media_by_status_id) =
            preload_local_timeline_rows_from_status_refs(&db, &local_status_refs).await?;
        let muted_actor_uris = preload_muted_timeline_actor_uris(
            &db,
            &config,
            &viewer,
            &local_status_refs,
            &remote_status_refs,
            &local_accounts_by_id,
        )
        .await?;
        (local_accounts_by_id, media_by_status_id, muted_actor_uris)
    };
    let muted_local_status_ids = muted_local_timeline_status_ids(
        &db,
        viewer.id(),
        viewer_has_thread_mutes,
        &local_statuses_by_id.values().collect::<Vec<_>>(),
    )
    .await?;
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
    let entries = timeline_entries_from_candidates(
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

    timeline_response_from_entries(&req, limit, entries)
}

pub(crate) async fn public_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: PublicTimelineQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
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
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;
    if timeline_cursor_is_unresolved(&pagination, &cursor) {
        return empty_timeline_response();
    }
    let filter_matcher = match viewer {
        Some(viewer) => Some(load_account_filter_matcher(&db, viewer.id()).await?),
        None => None,
    };
    let (local_statuses, remote_statuses) = futures_util::try_join!(
        async {
            if include_local {
                list_local_public_timeline_statuses(&db, &cursor, query_limit).await
            } else {
                Ok(Vec::new())
            }
        },
        async {
            if include_remote {
                list_remote_public_timeline_statuses(&db, &cursor, query_limit).await
            } else {
                Ok(Vec::new())
            }
        },
    )?;
    let only_media = query.only_media.unwrap_or(false);
    let mut candidates = Vec::new();
    let mut local_accounts_by_id = HashMap::new();
    let viewer_has_thread_mutes = match viewer {
        Some(viewer) => account_has_thread_mutes(&db, viewer.id()).await?,
        None => false,
    };

    if include_local {
        let (accounts_by_id, mut media_by_status_id) =
            preload_local_timeline_rows(&db, &local_statuses).await?;
        local_accounts_by_id = accounts_by_id;
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
    let entries = timeline_entries_from_candidates(
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

    timeline_response_from_entries(&req, limit, entries)
}

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
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;
    if timeline_cursor_is_unresolved(&pagination, &cursor) {
        return empty_timeline_response();
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
    let entries = timeline_entries_from_candidates(
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
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
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
        .filter_map(|url| canonicalize_link_timeline_url(url))
        .collect::<HashSet<_>>();
    let viewer = access.viewer();
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;
    if timeline_cursor_is_unresolved(&pagination, &cursor) {
        return empty_timeline_response();
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
        if !status_card_url_matches_targets(&strip_html_tags(&status.content_html), &target_url_set)
        {
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
    let entries = timeline_entries_from_candidates(
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

    timeline_response_from_entries(&req, limit, entries)
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
            let text = crate::strip_html_tags(&status.content_html);
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
    let entries = timeline_entries_from_candidates(
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
                    text: String::new(),
                    spoiler_text: String::new(),
                    visibility: cfwdon_domain::Visibility::Public,
                    sensitive: false,
                    language: None,
                    quote_approval_policy: None,
                    quote_state: cfwdon_domain::QuoteState::Accepted,
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
