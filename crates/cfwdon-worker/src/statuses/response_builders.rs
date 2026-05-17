use super::reblog_response::{
    local_reblog_wrapper_response_from_embedded, remote_reblog_wrapper_response_from_embedded,
};
use super::{
    AccountFilterMatcher, AccountRow, AppConfig, LocalAccount, LocalStatusResponseDetails,
    MastodonPollResponsePreload, MastodonStatusResponse, MediaAttachmentRow, RemoteActorRow,
    RemoteMastodonPollResponsePreload, RemoteStatusAttachmentRow, RemoteStatusEditUpdatedAtPreload,
    RemoteStatusResponseDetails, RemoteStatusRow, StatusCountsPreload, StatusRow,
    account_has_thread_mutes, actor_url, build_remote_status_card_value, build_status_card_value,
    count_rows, effective_remote_status_quote_state, effective_status_quote_state,
    find_local_status_by_object_uri, find_oauth_app_by_id, find_oauth_apps_by_ids,
    find_remote_actor_by_actor_uri, find_remote_status_attachments_by_status_id,
    find_remote_status_by_url_or_object_uri, has_remote_status_edit_snapshots, is_blocking_actor,
    is_local_follower_authorized, is_local_status_bookmarked_by, is_local_status_favourited_by,
    is_local_status_pinned_by, is_local_status_reblogged_by, is_local_status_thread_muted_by,
    is_muted_actor, is_remote_status_bookmarked_by, is_remote_status_favourited_by,
    is_remote_status_reblogged_by, load_local_status_counts, load_local_status_response_preload,
    load_mastodon_poll_response, load_remote_mastodon_poll_response, load_remote_status_counts,
    load_remote_status_updated_at, load_status_filtered, load_status_updated_at,
    local_status_target_uri, resolve_local_status_response_subject, strip_html_tags,
};
use cfwdon_domain::AccountHandle;
use std::collections::{HashMap, HashSet};
use worker::{D1Database, Result, d1::D1Type};

async fn build_status_application(
    db: &D1Database,
    application_id: Option<i64>,
) -> Result<Option<serde_json::Value>> {
    let Some(application_id) = application_id else {
        return Ok(None);
    };
    let Some(app) = find_oauth_app_by_id(db, application_id).await? else {
        return Ok(None);
    };
    Ok(Some(serde_json::json!({
        "name": app.name,
        "website": app.website,
    })))
}

fn status_application_value(name: String, website: Option<String>) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "website": website,
    })
}

#[derive(Debug, Default)]
pub(crate) struct StatusApplicationPreload {
    applications: HashMap<i64, serde_json::Value>,
}

impl StatusApplicationPreload {
    fn application(&self, application_id: Option<i64>) -> Option<serde_json::Value> {
        application_id.and_then(|id| self.applications.get(&id).cloned())
    }
}

pub(crate) async fn preload_status_applications(
    db: &D1Database,
    statuses: &[&StatusRow],
) -> Result<StatusApplicationPreload> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for status in statuses {
        if let Some(application_id) = status.application_id
            && seen.insert(application_id)
        {
            ids.push(application_id);
        }
    }

    let applications = find_oauth_apps_by_ids(db, &ids)
        .await?
        .into_iter()
        .map(|app| (app.id, status_application_value(app.name, app.website)))
        .collect();

    Ok(StatusApplicationPreload { applications })
}

async fn local_status_edited_at(db: &D1Database, status: &StatusRow) -> Result<Option<String>> {
    let updated_at = match status.updated_at.as_deref() {
        Some(updated_at) => Some(updated_at.to_owned()),
        None => load_status_updated_at(db, &status.id).await?,
    };
    Ok(local_status_edited_at_from_updated_at(
        &status.created_at,
        updated_at,
    ))
}

fn local_status_edited_at_from_updated_at(
    created_at: &str,
    updated_at: Option<String>,
) -> Option<String> {
    updated_at.filter(|updated_at| updated_at != created_at)
}

pub(crate) fn quote_document_with_state(
    state: &str,
    quoted_status: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "state": state,
        "quoted_status": quoted_status,
    })
}

pub(crate) fn pending_quote_document() -> serde_json::Value {
    quote_placeholder_document("pending")
}

pub(crate) fn quote_placeholder_document(state: &str) -> serde_json::Value {
    serde_json::json!({
        "state": state,
        "quoted_status": serde_json::Value::Null,
    })
}

fn unauthorized_quote_document() -> serde_json::Value {
    quote_placeholder_document("unauthorized")
}

fn quote_state_uses_placeholder(state: &str) -> bool {
    matches!(state, "revoked" | "rejected" | "unauthorized" | "deleted")
}

fn quote_document_for_local_state(local_quote_state: Option<&str>) -> Option<serde_json::Value> {
    match local_quote_state {
        Some("pending") => Some(pending_quote_document()),
        Some(state) if quote_state_uses_placeholder(state) => {
            Some(quote_placeholder_document(state))
        }
        _ => None,
    }
}

fn quote_document_from_response(
    state: &str,
    response: MastodonStatusResponse,
) -> serde_json::Value {
    quote_document_with_state(
        state,
        serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
    )
}

fn accepted_status_quotes_count_sql() -> &'static str {
    // Keep local and remote quote counts in one D1 round trip while preserving
    // separate table indexes on quote_of_uri.
    "SELECT COALESCE(SUM(count), 0) AS count
     FROM (
         SELECT COUNT(*) AS count
         FROM statuses
         WHERE quote_of_uri = ?1
           AND quote_state = 'accepted'
         UNION ALL
         SELECT COUNT(*) AS count
         FROM remote_statuses
         WHERE quote_of_uri = ?1
           AND quote_state = 'accepted'
     )"
}

#[derive(Debug, serde::Deserialize)]
struct StatusQuoteCountRow {
    quote_of_uri: String,
    count: u64,
}

#[derive(Debug, serde::Deserialize)]
struct TargetUriRow {
    target_uri: String,
}

#[derive(Debug, serde::Deserialize)]
struct StatusIdRow {
    status_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct RemoteStatusIdRow {
    remote_status_id: String,
}

#[derive(Debug, Default)]
pub(crate) struct StatusQuoteCountsPreload {
    counts: HashMap<String, u64>,
    preloaded_uris: HashSet<String>,
}

impl StatusQuoteCountsPreload {
    fn count(&self, status_uri: &str) -> Option<u64> {
        self.preloaded_uris
            .contains(status_uri)
            .then(|| self.counts.get(status_uri).copied().unwrap_or(0))
    }
}

#[derive(Debug, Default)]
pub(crate) struct LocalStatusViewerStatePreload {
    favourited_target_uris: HashSet<String>,
    reblogged_target_uris: HashSet<String>,
    bookmarked_target_uris: HashSet<String>,
    pinned_status_ids: HashSet<String>,
    has_thread_mutes: bool,
}

impl LocalStatusViewerStatePreload {
    fn favourited(&self, status: &StatusRow) -> bool {
        self.favourited_target_uris
            .contains(&local_status_target_uri(status))
    }

    fn reblogged(&self, status: &StatusRow) -> bool {
        self.reblogged_target_uris
            .contains(&local_status_target_uri(status))
    }

    fn bookmarked(&self, status: &StatusRow) -> bool {
        self.bookmarked_target_uris
            .contains(&local_status_target_uri(status))
    }

    fn pinned(&self, status_id: &str) -> bool {
        self.pinned_status_ids.contains(status_id)
    }

    fn can_skip_thread_mute_lookup(&self) -> bool {
        !self.has_thread_mutes
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct LocalStatusPreloadedViewerState {
    favourited: bool,
    reblogged: bool,
    bookmarked: bool,
    pinned: bool,
    muted: Option<bool>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct LocalStatusResponseViewerState {
    favourited: bool,
    reblogged: bool,
    bookmarked: bool,
    pinned: bool,
    muted: bool,
}

fn preloaded_local_status_response_viewer_state(
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    preload: Option<&LocalStatusViewerStatePreload>,
) -> Option<LocalStatusPreloadedViewerState> {
    match (viewer, preload) {
        (Some(_), Some(preload)) => Some(LocalStatusPreloadedViewerState {
            favourited: preload.favourited(status),
            reblogged: preload.reblogged(status),
            bookmarked: preload.bookmarked(status),
            pinned: preload.pinned(&status.id),
            muted: preload.can_skip_thread_mute_lookup().then_some(false),
        }),
        (None, _) => Some(LocalStatusPreloadedViewerState {
            muted: Some(false),
            ..LocalStatusPreloadedViewerState::default()
        }),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub(crate) struct RemoteStatusViewerStatePreload {
    favourited_status_ids: HashSet<String>,
    reblogged_status_ids: HashSet<String>,
    bookmarked_status_ids: HashSet<String>,
    muted_actor_uris: HashSet<String>,
}

impl RemoteStatusViewerStatePreload {
    fn favourited(&self, status_id: &str) -> bool {
        self.favourited_status_ids.contains(status_id)
    }

    fn reblogged(&self, status_id: &str) -> bool {
        self.reblogged_status_ids.contains(status_id)
    }

    fn bookmarked(&self, status_id: &str) -> bool {
        self.bookmarked_status_ids.contains(status_id)
    }

    fn muted(&self, actor_uri: &str) -> bool {
        self.muted_actor_uris.contains(actor_uri)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RemoteStatusResponseViewerState {
    favourited: bool,
    reblogged: bool,
    bookmarked: bool,
    muted: bool,
}

fn preloaded_remote_status_response_viewer_state(
    viewer: Option<&LocalAccount>,
    status_id: &str,
    actor_uri: &str,
    preload: Option<&RemoteStatusViewerStatePreload>,
) -> Option<RemoteStatusResponseViewerState> {
    match (viewer, preload) {
        (Some(_), Some(preload)) => Some(RemoteStatusResponseViewerState {
            favourited: preload.favourited(status_id),
            reblogged: preload.reblogged(status_id),
            bookmarked: preload.bookmarked(status_id),
            muted: preload.muted(actor_uri),
        }),
        (None, _) => Some(RemoteStatusResponseViewerState::default()),
        _ => None,
    }
}

async fn load_viewer_target_uri_set(
    db: &D1Database,
    table: &str,
    account_id: &str,
    target_uris: &[String],
) -> Result<HashSet<String>> {
    if target_uris.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders = (2..=(target_uris.len() + 1))
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT target_uri
         FROM {table}
         WHERE account_id = ?1
           AND target_uri IN ({placeholders})"
    );
    let mut bindings = Vec::with_capacity(target_uris.len() + 1);
    bindings.push(D1Type::Text(account_id));
    bindings.extend(target_uris.iter().map(|uri| D1Type::Text(uri.as_str())));
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<TargetUriRow>()?
        .into_iter()
        .map(|row| row.target_uri)
        .collect())
}

async fn load_viewer_remote_status_id_set(
    db: &D1Database,
    table: &str,
    account_id: &str,
    status_ids: &[String],
) -> Result<HashSet<String>> {
    if status_ids.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders = (2..=(status_ids.len() + 1))
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT remote_status_id
         FROM {table}
         WHERE account_id = ?1
           AND remote_status_id IN ({placeholders})"
    );
    let mut bindings = Vec::with_capacity(status_ids.len() + 1);
    bindings.push(D1Type::Text(account_id));
    bindings.extend(status_ids.iter().map(|id| D1Type::Text(id.as_str())));
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<RemoteStatusIdRow>()?
        .into_iter()
        .map(|row| row.remote_status_id)
        .collect())
}

async fn load_viewer_pinned_status_ids(
    db: &D1Database,
    account_id: &str,
    status_ids: &[String],
) -> Result<HashSet<String>> {
    if status_ids.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders = (2..=(status_ids.len() + 1))
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT status_id
         FROM status_pins
         WHERE account_id = ?1
           AND status_id IN ({placeholders})"
    );
    let mut bindings = Vec::with_capacity(status_ids.len() + 1);
    bindings.push(D1Type::Text(account_id));
    bindings.extend(status_ids.iter().map(|id| D1Type::Text(id.as_str())));
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<StatusIdRow>()?
        .into_iter()
        .map(|row| row.status_id)
        .collect())
}

pub(crate) async fn preload_local_status_viewer_state(
    db: &D1Database,
    account_id: &str,
    statuses: &[&StatusRow],
    known_has_thread_mutes: Option<bool>,
) -> Result<LocalStatusViewerStatePreload> {
    let mut seen_targets = HashSet::new();
    let target_uris = statuses
        .iter()
        .map(|status| local_status_target_uri(status))
        .filter(|uri| seen_targets.insert(uri.clone()))
        .collect::<Vec<_>>();
    let mut seen_status_ids = HashSet::new();
    let status_ids = statuses
        .iter()
        .map(|status| status.id.clone())
        .filter(|id| seen_status_ids.insert(id.clone()))
        .collect::<Vec<_>>();

    let (
        favourited_target_uris,
        reblogged_target_uris,
        bookmarked_target_uris,
        pinned_status_ids,
        has_thread_mutes,
    ) = futures_util::try_join!(
        load_viewer_target_uri_set(db, "favourites", account_id, &target_uris,),
        load_viewer_target_uri_set(db, "reblogs", account_id, &target_uris),
        load_viewer_target_uri_set(db, "bookmarks", account_id, &target_uris,),
        load_viewer_pinned_status_ids(db, account_id, &status_ids),
        async {
            match known_has_thread_mutes {
                Some(value) => Ok(value),
                None => account_has_thread_mutes(db, account_id).await,
            }
        },
    )?;

    Ok(LocalStatusViewerStatePreload {
        favourited_target_uris,
        reblogged_target_uris,
        bookmarked_target_uris,
        pinned_status_ids,
        has_thread_mutes,
    })
}

pub(crate) async fn preload_remote_status_viewer_state(
    db: &D1Database,
    account_id: &str,
    statuses: &[(&RemoteStatusRow, &RemoteActorRow)],
) -> Result<RemoteStatusViewerStatePreload> {
    let mut seen_status_ids = HashSet::new();
    let status_ids = statuses
        .iter()
        .map(|(status, _)| status.id.clone())
        .filter(|id| seen_status_ids.insert(id.clone()))
        .collect::<Vec<_>>();
    let mut seen_actor_uris = HashSet::new();
    let actor_uris = statuses
        .iter()
        .map(|(_, actor)| actor.actor_uri.clone())
        .filter(|uri| seen_actor_uris.insert(uri.clone()))
        .collect::<Vec<_>>();

    let (favourited_status_ids, reblogged_status_ids, bookmarked_status_ids, muted_actor_uris) = futures_util::try_join!(
        load_viewer_remote_status_id_set(db, "favourites", account_id, &status_ids),
        load_viewer_remote_status_id_set(db, "reblogs", account_id, &status_ids),
        load_viewer_remote_status_id_set(db, "bookmarks", account_id, &status_ids),
        crate::list_active_muted_actor_uris(db, account_id, &actor_uris),
    )?;

    Ok(RemoteStatusViewerStatePreload {
        favourited_status_ids,
        reblogged_status_ids,
        bookmarked_status_ids,
        muted_actor_uris,
    })
}

pub(crate) async fn preload_status_quote_counts(
    db: &D1Database,
    status_uris: &[String],
) -> Result<StatusQuoteCountsPreload> {
    let mut seen = HashSet::new();
    let uris = status_uris
        .iter()
        .filter(|uri| seen.insert(uri.as_str()))
        .collect::<Vec<_>>();
    if uris.is_empty() {
        return Ok(StatusQuoteCountsPreload::default());
    }
    let preloaded_uris = uris
        .iter()
        .map(|uri| (*uri).clone())
        .collect::<HashSet<_>>();

    let placeholders = (1..=uris.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT quote_of_uri, SUM(count) AS count
         FROM (
             SELECT quote_of_uri, COUNT(*) AS count
             FROM statuses
             WHERE quote_state = 'accepted'
               AND quote_of_uri IN ({placeholders})
             GROUP BY quote_of_uri
             UNION ALL
             SELECT quote_of_uri, COUNT(*) AS count
             FROM remote_statuses
             WHERE quote_state = 'accepted'
               AND quote_of_uri IN ({placeholders})
             GROUP BY quote_of_uri
         )
         GROUP BY quote_of_uri"
    );
    let bindings = uris
        .iter()
        .map(|uri| D1Type::Text(uri.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
    let counts = result
        .results::<StatusQuoteCountRow>()?
        .into_iter()
        .map(|row| (row.quote_of_uri, row.count))
        .collect::<HashMap<_, _>>();

    Ok(StatusQuoteCountsPreload {
        counts,
        preloaded_uris,
    })
}

async fn count_status_quotes_by_uri(db: &D1Database, status_uri: &str) -> Result<u64> {
    count_rows(db, accepted_status_quotes_count_sql(), status_uri).await
}

async fn local_status_poll_response(
    db: &D1Database,
    poll_preload: Option<&MastodonPollResponsePreload>,
    status_id: &str,
    viewer: Option<&LocalAccount>,
) -> Result<Option<serde_json::Value>> {
    if let Some(poll) = poll_preload.and_then(|preload| preload.poll_response(status_id)) {
        return Ok(poll);
    }

    load_mastodon_poll_response(db, status_id, viewer).await
}

async fn status_quotes_count(
    db: &D1Database,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    status_uri: &str,
) -> Result<u64> {
    if let Some(count) = quote_counts_preload.and_then(|counts| counts.count(status_uri)) {
        return Ok(count);
    }

    count_status_quotes_by_uri(db, status_uri).await
}

async fn viewer_blocks_domain(db: &D1Database, account_id: &str, domain: &str) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(domain)];
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM account_domain_blocks
             WHERE account_id = ?1
               AND domain = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
}

async fn quote_state_for_local_quoted_status(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    quoted_account: &LocalAccount,
) -> Result<Option<&'static str>> {
    let quoted_actor_uri = actor_url(config, &quoted_account.username);
    if is_blocking_actor(db, &viewer.id, &quoted_actor_uri).await? {
        return Ok(Some("blocked_account"));
    }
    if is_muted_actor(db, &viewer.id, &quoted_actor_uri).await? {
        return Ok(Some("muted_account"));
    }
    Ok(None)
}

async fn quote_state_for_remote_quoted_status(
    db: &D1Database,
    viewer: &LocalAccount,
    actor: &RemoteActorRow,
) -> Result<Option<&'static str>> {
    if is_blocking_actor(db, &viewer.id, &actor.actor_uri).await? {
        return Ok(Some("blocked_account"));
    }
    if viewer_blocks_domain(db, &viewer.id, &actor.domain).await? {
        return Ok(Some("blocked_domain"));
    }
    if is_muted_actor(db, &viewer.id, &actor.actor_uri).await? {
        return Ok(Some("muted_account"));
    }
    Ok(None)
}

fn remote_media_attachment_values(
    attachments: &[crate::RemoteStatusAttachmentRow],
) -> Vec<serde_json::Value> {
    attachments
        .iter()
        .map(|media| {
            serde_json::to_value(crate::MastodonMediaAttachmentResponse::from_remote_row(
                media,
            ))
            .unwrap_or(serde_json::Value::Null)
        })
        .collect()
}

fn remote_quote_visibility_is_embeddable(visibility: &str) -> bool {
    matches!(visibility, "public" | "unlisted")
}

fn accepted_quote_document_state() -> &'static str {
    "accepted"
}

async fn local_quoted_status_document_state(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    local_account: &LocalAccount,
) -> Result<&'static str> {
    let Some(viewer) = viewer else {
        return Ok(accepted_quote_document_state());
    };
    Ok(
        quote_state_for_local_quoted_status(db, config, viewer, local_account)
            .await?
            .unwrap_or(accepted_quote_document_state()),
    )
}

async fn remote_quoted_status_document_state(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    actor: &RemoteActorRow,
) -> Result<&'static str> {
    let Some(viewer) = viewer else {
        return Ok(accepted_quote_document_state());
    };
    Ok(quote_state_for_remote_quoted_status(db, viewer, actor)
        .await?
        .unwrap_or(accepted_quote_document_state()))
}

pub(crate) fn effective_local_quote_approval_policy(status: &StatusRow) -> &str {
    if matches!(status.visibility.as_str(), "private" | "direct") {
        "nobody"
    } else {
        status.quote_approval_policy.as_deref().unwrap_or("public")
    }
}

async fn build_local_quote_approval(
    db: &D1Database,
    status: &StatusRow,
    viewer: Option<&LocalAccount>,
    owner: &LocalAccount,
) -> Result<serde_json::Value> {
    let policy = effective_local_quote_approval_policy(status);
    let automatic = match policy {
        "public" => vec![serde_json::json!("public")],
        "followers" => vec![serde_json::json!("followers")],
        _ => Vec::new(),
    };
    let current_user = match policy {
        "public" => "automatic",
        "followers" => {
            if viewer.map(|viewer| viewer.id == owner.id).unwrap_or(false) {
                "automatic"
            } else if let Some(viewer) = viewer {
                if is_local_follower_authorized(db, &viewer.id, &owner.id).await? {
                    "automatic"
                } else {
                    "denied"
                }
            } else {
                "denied"
            }
        }
        _ => {
            if viewer.map(|viewer| viewer.id == owner.id).unwrap_or(false) {
                "automatic"
            } else {
                "denied"
            }
        }
    };

    Ok(serde_json::json!({
        "automatic": automatic,
        "manual": [],
        "current_user": current_user,
    }))
}

fn build_remote_quote_approval(status: &RemoteStatusRow) -> serde_json::Value {
    if !matches!(status.visibility.as_str(), "public" | "unlisted") {
        return serde_json::json!({
            "automatic": [],
            "manual": [],
            "current_user": "denied",
        });
    }

    serde_json::json!({
        "automatic": [],
        "manual": ["unsupported_policy"],
        "current_user": "manual",
    })
}

pub(crate) async fn build_status_mentions(
    db: &D1Database,
    config: &AppConfig,
    text: &str,
) -> Result<Vec<serde_json::Value>> {
    let handles = crate::extract_account_handles_from_text(text, config);
    if handles.is_empty() {
        return Ok(Vec::new());
    }

    let lookup_keys = mention_lookup_keys(&handles, &config.instance_domain);
    let local_accounts = load_mention_local_accounts(db, &lookup_keys.local_usernames).await?;
    let remote_actors = load_mention_remote_actors(db, &lookup_keys.remote_pairs).await?;

    Ok(handles
        .iter()
        .filter_map(|handle| {
            mention_document_for_handle(handle, config, &local_accounts, &remote_actors)
        })
        .collect())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MentionLookupKeys {
    local_usernames: Vec<String>,
    remote_pairs: Vec<(String, String)>,
}

fn mention_lookup_keys(handles: &[AccountHandle], instance_domain: &str) -> MentionLookupKeys {
    let mut keys = MentionLookupKeys::default();
    for handle in handles {
        if handle.is_local_to(instance_domain) {
            keys.local_usernames
                .push(handle.username.to_ascii_lowercase());
        } else if let Some(pair) = mention_remote_pair(handle) {
            keys.remote_pairs.push(pair);
        }
    }
    keys
}

fn mention_remote_pair(handle: &AccountHandle) -> Option<(String, String)> {
    handle.domain.as_deref().map(|domain| {
        (
            handle.username.to_ascii_lowercase(),
            domain.to_ascii_lowercase(),
        )
    })
}

fn mention_document_for_handle(
    handle: &AccountHandle,
    config: &AppConfig,
    local_accounts: &HashMap<String, LocalAccount>,
    remote_actors: &HashMap<(String, String), RemoteActorRow>,
) -> Option<serde_json::Value> {
    if handle.is_local_to(&config.instance_domain) {
        let account = local_accounts.get(&handle.username.to_ascii_lowercase())?;
        return Some(local_mention_document(config, account));
    }

    let key = mention_remote_pair(handle)?;
    let actor = remote_actors.get(&key)?;
    Some(remote_mention_document(actor))
}

fn local_mention_document(config: &AppConfig, account: &LocalAccount) -> serde_json::Value {
    serde_json::json!({
        "id": account.id.clone(),
        "username": account.username.clone(),
        "url": actor_url(config, &account.username),
        "acct": account.acct(),
    })
}

fn remote_mention_document(actor: &RemoteActorRow) -> serde_json::Value {
    serde_json::json!({
        "id": crate::remote_account_rest_id(&actor.actor_uri),
        "username": actor.username,
        "url": actor.profile_url.clone().unwrap_or_else(|| actor.actor_uri.clone()),
        "acct": format!("{}@{}", actor.username, actor.domain),
    })
}

async fn load_mention_local_accounts(
    db: &D1Database,
    usernames: &[String],
) -> Result<HashMap<String, LocalAccount>> {
    let usernames = crate::unique_ordered_refs(usernames);
    if usernames.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = crate::sql_placeholders(1, usernames.len());
    let sql = format!(
        "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
         FROM accounts
         WHERE lower(username) IN ({placeholders})"
    );
    let bindings = usernames
        .iter()
        .map(|username| D1Type::Text(username.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(|row| (row.username.to_ascii_lowercase(), LocalAccount::from(row)))
        .collect())
}

async fn load_mention_remote_actors(
    db: &D1Database,
    pairs: &[(String, String)],
) -> Result<HashMap<(String, String), RemoteActorRow>> {
    let mut seen = HashSet::new();
    let pairs = pairs
        .iter()
        .filter(|(username, domain)| seen.insert((username.as_str(), domain.as_str())))
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return Ok(HashMap::new());
    }

    let clauses = pairs
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let username = index * 2 + 1;
            let domain = username + 1;
            format!("(lower(username) = ?{username} AND lower(domain) = ?{domain})")
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!(
        "SELECT actor_uri, username, domain, created_at, locked, bot, discoverable, indexable, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE {clauses}"
    );
    let mut bindings = Vec::with_capacity(pairs.len() * 2);
    for (username, domain) in pairs {
        bindings.push(D1Type::Text(username.as_str()));
        bindings.push(D1Type::Text(domain.as_str()));
    }
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<RemoteActorRow>()?
        .into_iter()
        .map(|row| {
            (
                (
                    row.username.to_ascii_lowercase(),
                    row.domain.to_ascii_lowercase(),
                ),
                row,
            )
        })
        .collect())
}

pub(crate) async fn build_local_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_with_filter_matcher(
        db,
        config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        None,
    )
    .await
}

pub(crate) async fn build_loaded_local_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
) -> Result<MastodonStatusResponse> {
    let preload = load_local_status_response_preload(db, status).await?;
    build_local_status_response(
        db,
        config,
        viewer,
        status,
        account,
        preload.in_reply_to_account_id,
        preload.media,
    )
    .await
}

pub(crate) async fn build_local_status_response_with_filter_matcher(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_with_preloads(
        db,
        config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        filter_matcher,
        None,
    )
    .await
}

pub(crate) async fn build_local_status_response_with_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_with_timeline_preloads(
        db,
        config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        filter_matcher,
        counts_preload,
        None,
        None,
        None,
        None,
    )
    .await
}

pub(crate) async fn build_local_status_response_with_quote_count_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_with_timeline_preloads(
        db,
        config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        None,
    )
    .await
}

pub(crate) async fn build_local_status_response_with_timeline_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_inner(
        db,
        config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        true,
    )
    .await
}

async fn build_local_status_response_inner(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    if let Some(boost_of_uri) = status.boost_of_uri.as_deref() {
        return build_local_reblog_wrapper_response(
            db,
            config,
            viewer,
            status,
            account,
            in_reply_to_account_id,
            boost_of_uri,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            poll_preload,
            viewer_state_preload,
            application_preload,
            include_quote,
        )
        .await;
    }

    let mut response = MastodonStatusResponse::from_row(
        status,
        account,
        config,
        in_reply_to_account_id,
        media_attachments,
    );
    let details = load_local_status_response_details(
        db,
        config,
        viewer,
        status,
        account,
        &response.uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        include_quote,
    )
    .await?;
    response.apply_local_details(details);
    Ok(response)
}

async fn load_local_status_response_details(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    status_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    include_quote: bool,
) -> Result<LocalStatusResponseDetails> {
    let application = match application_preload {
        Some(preload) => preload.application(status.application_id),
        None => build_status_application(db, status.application_id).await?,
    };
    let card = build_status_card_value(&status._text_content);
    let poll = local_status_poll_response(db, poll_preload, &status.id, viewer).await?;
    let mentions = build_status_mentions(db, config, &status._text_content).await?;
    let (favourites_count, reblogs_count) =
        local_status_counts(db, counts_preload, &status.id).await?;
    let quotes_count = status_quotes_count(db, quote_counts_preload, status_uri).await?;
    let viewer_state =
        local_status_response_viewer_state(db, viewer, status, viewer_state_preload).await?;
    let edited_at = local_status_edited_at(db, status).await?;
    let filtered = local_status_filtered_for_viewer(db, viewer, status, filter_matcher).await?;
    let quote_approval = Some(build_local_quote_approval(db, status, viewer, account).await?);
    let quote = if include_quote {
        build_quoted_status_value(
            db,
            config,
            viewer,
            status.quote_of_uri.as_deref(),
            Some(effective_status_quote_state(status)),
            true,
            filter_matcher,
            counts_preload,
        )
        .await?
    } else {
        None
    };

    Ok(LocalStatusResponseDetails {
        application,
        card,
        poll,
        mentions,
        favourites_count,
        reblogs_count,
        quotes_count,
        favourited: viewer_state.favourited,
        reblogged: viewer_state.reblogged,
        muted: viewer_state.muted,
        bookmarked: viewer_state.bookmarked,
        pinned: viewer_state.pinned,
        edited_at,
        filtered,
        quote_approval,
        quote,
    })
}

async fn local_status_response_viewer_state(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    preload: Option<&LocalStatusViewerStatePreload>,
) -> Result<LocalStatusResponseViewerState> {
    if let Some(state) = preloaded_local_status_response_viewer_state(viewer, status, preload) {
        if let Some(muted) = state.muted {
            return Ok(LocalStatusResponseViewerState {
                favourited: state.favourited,
                reblogged: state.reblogged,
                bookmarked: state.bookmarked,
                pinned: state.pinned,
                muted,
            });
        }

        let Some(viewer) = viewer else {
            return Ok(LocalStatusResponseViewerState::default());
        };
        return Ok(LocalStatusResponseViewerState {
            favourited: state.favourited,
            reblogged: state.reblogged,
            bookmarked: state.bookmarked,
            pinned: state.pinned,
            muted: is_local_status_thread_muted_by(db, &viewer.id, status).await?,
        });
    }

    let Some(viewer) = viewer else {
        return Ok(LocalStatusResponseViewerState::default());
    };

    Ok(LocalStatusResponseViewerState {
        favourited: is_local_status_favourited_by(db, &viewer.id, status).await?,
        reblogged: is_local_status_reblogged_by(db, &viewer.id, status).await?,
        bookmarked: is_local_status_bookmarked_by(db, &viewer.id, status).await?,
        pinned: is_local_status_pinned_by(db, &viewer.id, &status.id).await?,
        muted: is_local_status_thread_muted_by(db, &viewer.id, status).await?,
    })
}

pub(crate) async fn build_remote_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_with_filter_matcher(db, config, viewer, status, actor, None).await
}

pub(crate) async fn build_remote_status_response_with_filter_matcher(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    filter_matcher: Option<&AccountFilterMatcher>,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_with_preloads(
        db,
        config,
        viewer,
        status,
        actor,
        filter_matcher,
        None,
    )
    .await
}

pub(crate) async fn build_remote_status_response_with_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_inner(
        db,
        config,
        viewer,
        status,
        actor,
        filter_matcher,
        counts_preload,
        None,
        None,
        None,
        None,
        None,
        true,
    )
    .await
}

pub(crate) async fn build_remote_status_response_with_timeline_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    remote_attachments: Vec<RemoteStatusAttachmentRow>,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_inner(
        db,
        config,
        viewer,
        status,
        actor,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        viewer_state_preload,
        poll_preload,
        edit_updated_at_preload,
        Some(remote_attachments),
        true,
    )
    .await
}

async fn build_remote_status_response_inner(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    remote_attachments: Option<Vec<RemoteStatusAttachmentRow>>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    if let Some(boost_of_uri) = status.boost_of_uri.as_deref() {
        return build_remote_reblog_wrapper_response(
            db,
            config,
            viewer,
            status,
            actor,
            boost_of_uri,
            filter_matcher,
            counts_preload,
            include_quote,
        )
        .await;
    }

    let mut response = MastodonStatusResponse::from_remote_row(status, actor, config);
    let details = load_remote_status_response_details(
        db,
        config,
        viewer,
        status,
        actor,
        &response.uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        viewer_state_preload,
        poll_preload,
        edit_updated_at_preload,
        remote_attachments,
        include_quote,
    )
    .await?;
    response.apply_remote_details(details);
    Ok(response)
}

async fn load_remote_status_response_details(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    status_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    remote_attachments: Option<Vec<RemoteStatusAttachmentRow>>,
    include_quote: bool,
) -> Result<RemoteStatusResponseDetails> {
    let text_content = strip_html_tags(&status.content_html);
    let remote_attachments = match remote_attachments {
        Some(attachments) => attachments,
        None => find_remote_status_attachments_by_status_id(db, &status.id).await?,
    };
    let card = build_remote_status_card_value(&text_content, &remote_attachments);
    let media_attachments = remote_media_attachment_values(&remote_attachments);
    let mentions = build_status_mentions(db, config, &text_content).await?;
    let (favourites_count, reblogs_count) =
        remote_status_counts(db, counts_preload, &status.id).await?;
    let quotes_count = status_quotes_count(db, quote_counts_preload, status_uri).await?;
    let viewer_state =
        remote_status_response_viewer_state(db, viewer, status, actor, viewer_state_preload)
            .await?;
    let poll = match poll_preload {
        Some(preload) => preload.poll_response(&status.id),
        None => load_remote_mastodon_poll_response(db, status, viewer).await?,
    };
    let edited_at = match edit_updated_at_preload {
        Some(preload) => preload.updated_at(&status.id).map(ToOwned::to_owned),
        None => {
            if has_remote_status_edit_snapshots(db, &status.id).await? {
                load_remote_status_updated_at(db, &status.id).await?
            } else {
                None
            }
        }
    };
    let filtered =
        remote_status_filtered_for_viewer(db, viewer, status, &text_content, filter_matcher)
            .await?;
    let quote_approval = Some(build_remote_quote_approval(status));
    let quote = if include_quote {
        build_quoted_status_value(
            db,
            config,
            viewer,
            status.quote_of_uri.as_deref(),
            Some(effective_remote_status_quote_state(status)),
            false,
            filter_matcher,
            counts_preload,
        )
        .await?
    } else {
        None
    };

    Ok(RemoteStatusResponseDetails {
        media_attachments,
        card,
        poll,
        mentions,
        favourites_count,
        reblogs_count,
        quotes_count,
        favourited: viewer_state.favourited,
        reblogged: viewer_state.reblogged,
        muted: viewer_state.muted,
        bookmarked: viewer_state.bookmarked,
        edited_at,
        filtered,
        quote_approval,
        quote,
    })
}

async fn remote_status_response_viewer_state(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    preload: Option<&RemoteStatusViewerStatePreload>,
) -> Result<RemoteStatusResponseViewerState> {
    if let Some(state) =
        preloaded_remote_status_response_viewer_state(viewer, &status.id, &actor.actor_uri, preload)
    {
        return Ok(state);
    }

    let Some(viewer) = viewer else {
        return Ok(RemoteStatusResponseViewerState::default());
    };

    Ok(RemoteStatusResponseViewerState {
        favourited: is_remote_status_favourited_by(db, &viewer.id, &status.id).await?,
        reblogged: is_remote_status_reblogged_by(db, &viewer.id, &status.id).await?,
        bookmarked: is_remote_status_bookmarked_by(db, &viewer.id, &status.id).await?,
        muted: is_muted_actor(db, &viewer.id, &actor.actor_uri).await?,
    })
}

async fn build_quoted_status_value(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: Option<&str>,
    local_quote_state: Option<&str>,
    pending_remote_quote: bool,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok(None);
    };
    if let Some(document) = quote_document_for_local_state(local_quote_state) {
        return Ok(Some(document));
    }

    if let Some(document) = build_local_quoted_status_document(
        db,
        config,
        viewer,
        quote_of_uri,
        filter_matcher,
        counts_preload,
    )
    .await?
    {
        return Ok(Some(document));
    }

    build_remote_quoted_status_document(
        db,
        config,
        viewer,
        quote_of_uri,
        pending_remote_quote,
        filter_matcher,
        counts_preload,
    )
    .await
}

async fn build_local_quoted_status_document(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    if let Some(local_status) = find_local_status_by_object_uri(db, config, quote_of_uri).await? {
        let Some(subject) = resolve_local_status_response_subject(db, viewer, local_status).await?
        else {
            return Ok(None);
        };
        let super::ResolvedLocalStatusResponseSubject::Loaded(subject) = subject else {
            return Ok(Some(unauthorized_quote_document()));
        };
        let super::LoadedLocalStatusResponseSubject {
            status: local_status,
            account: local_account,
            preload:
                super::LocalStatusResponsePreload {
                    media,
                    in_reply_to_account_id,
                },
        } = subject;
        let mut response = MastodonStatusResponse::from_row(
            &local_status,
            &local_account,
            config,
            in_reply_to_account_id,
            media,
        );
        response.card = build_status_card_value(&local_status._text_content);
        response.poll = load_mastodon_poll_response(db, &local_status.id, viewer).await?;
        response.filtered =
            local_status_filtered_for_viewer(db, viewer, &local_status, filter_matcher).await?;
        response.mentions = build_status_mentions(db, config, &local_status._text_content).await?;
        let (favourites_count, reblogs_count) =
            local_status_counts(db, counts_preload, &local_status.id).await?;
        response.favourites_count = favourites_count;
        response.reblogs_count = reblogs_count;
        let viewer_state =
            local_status_response_viewer_state(db, viewer, &local_status, None).await?;
        response.favourited = viewer_state.favourited;
        response.reblogged = viewer_state.reblogged;
        response.bookmarked = viewer_state.bookmarked;
        response.pinned = viewer_state.pinned;
        response.muted = viewer_state.muted;
        response.quote = None;
        let state = local_quoted_status_document_state(db, config, viewer, &local_account).await?;
        return Ok(Some(quote_document_from_response(state, response)));
    }

    Ok(None)
}

async fn build_remote_quoted_status_document(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: &str,
    pending_remote_quote: bool,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    if let Some(remote_status) = find_remote_status_by_url_or_object_uri(db, quote_of_uri).await? {
        if pending_remote_quote {
            return Ok(Some(pending_quote_document()));
        }
        if !remote_quote_visibility_is_embeddable(&remote_status.visibility) {
            return Ok(Some(unauthorized_quote_document()));
        }
        let Some(actor) = find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await?
        else {
            return Ok(None);
        };
        let mut response = MastodonStatusResponse::from_remote_row(&remote_status, &actor, config);
        let text_content = strip_html_tags(&remote_status.content_html);
        let remote_attachments =
            find_remote_status_attachments_by_status_id(db, &remote_status.id).await?;
        response.card = build_remote_status_card_value(&text_content, &remote_attachments);
        response.media_attachments = remote_media_attachment_values(&remote_attachments);
        response.filtered = remote_status_filtered_for_viewer(
            db,
            viewer,
            &remote_status,
            &text_content,
            filter_matcher,
        )
        .await?;
        response.mentions = build_status_mentions(db, config, &text_content).await?;
        let (favourites_count, reblogs_count) =
            remote_status_counts(db, counts_preload, &remote_status.id).await?;
        response.favourites_count = favourites_count;
        response.reblogs_count = reblogs_count;
        let viewer_state =
            remote_status_response_viewer_state(db, viewer, &remote_status, &actor, None).await?;
        response.favourited = viewer_state.favourited;
        response.reblogged = viewer_state.reblogged;
        response.bookmarked = viewer_state.bookmarked;
        response.muted = viewer_state.muted;
        response.poll = load_remote_mastodon_poll_response(db, &remote_status, viewer).await?;
        response.quote = None;
        let state = remote_quoted_status_document_state(db, viewer, &actor).await?;
        return Ok(Some(quote_document_from_response(state, response)));
    }

    Ok(None)
}

async fn local_status_filtered_for_viewer(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
) -> Result<Vec<serde_json::Value>> {
    let Some(viewer) = viewer else {
        return Ok(Vec::new());
    };
    filtered_status_for_viewer(
        db,
        filter_matcher,
        &viewer.id,
        &status.id,
        &status._text_content,
        &status.spoiler_text,
    )
    .await
}

async fn remote_status_filtered_for_viewer(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    text_content: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
) -> Result<Vec<serde_json::Value>> {
    let Some(viewer) = viewer else {
        return Ok(Vec::new());
    };
    filtered_status_for_viewer(
        db,
        filter_matcher,
        &viewer.id,
        &status.id,
        text_content,
        &status.spoiler_text,
    )
    .await
}

async fn filtered_status_for_viewer(
    db: &D1Database,
    filter_matcher: Option<&AccountFilterMatcher>,
    account_id: &str,
    status_id: &str,
    text: &str,
    spoiler_text: &str,
) -> Result<Vec<serde_json::Value>> {
    if let Some(filter_matcher) = filter_matcher {
        return Ok(filter_matcher.filtered_status(status_id, text, spoiler_text));
    }

    load_status_filtered(db, account_id, status_id, text, spoiler_text).await
}

async fn local_status_counts(
    db: &D1Database,
    counts_preload: Option<&StatusCountsPreload>,
    status_id: &str,
) -> Result<(u64, u64)> {
    if let Some(counts) = counts_preload.and_then(|counts| counts.local_counts(status_id)) {
        return Ok(counts);
    }

    load_local_status_counts(db, status_id).await
}

async fn remote_status_counts(
    db: &D1Database,
    counts_preload: Option<&StatusCountsPreload>,
    status_id: &str,
) -> Result<(u64, u64)> {
    if let Some(counts) = counts_preload.and_then(|counts| counts.remote_counts(status_id)) {
        return Ok(counts);
    }

    load_remote_status_counts(db, status_id).await
}

async fn build_remote_reblog_wrapper_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    wrapper_status: &RemoteStatusRow,
    wrapper_actor: &RemoteActorRow,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    let embedded = build_reblog_embedded_response(
        db,
        config,
        viewer,
        boost_of_uri,
        filter_matcher,
        counts_preload,
        None,
        None,
        None,
        None,
        include_quote,
    )
    .await?;

    Ok(remote_reblog_wrapper_response_from_embedded(
        embedded,
        wrapper_status,
        wrapper_actor,
        config,
    ))
}

async fn build_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    if let Some(local_status) = find_local_status_by_object_uri(db, config, boost_of_uri).await? {
        return build_local_reblog_embedded_response(
            db,
            config,
            viewer,
            local_status,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            poll_preload,
            viewer_state_preload,
            application_preload,
            include_quote,
        )
        .await;
    }

    if let Some(remote_status) = find_remote_status_by_url_or_object_uri(db, boost_of_uri).await? {
        return build_remote_reblog_embedded_response(
            db,
            config,
            viewer,
            remote_status,
            filter_matcher,
            counts_preload,
            include_quote,
        )
        .await;
    }

    Ok(None)
}

async fn build_local_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    local_status: StatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    let Some(subject) = resolve_local_status_response_subject(db, viewer, local_status).await?
    else {
        return Ok(None);
    };
    let super::ResolvedLocalStatusResponseSubject::Loaded(subject) = subject else {
        return Ok(None);
    };
    let super::LoadedLocalStatusResponseSubject {
        status: local_status,
        account: local_account,
        preload:
            super::LocalStatusResponsePreload {
                media,
                in_reply_to_account_id,
            },
    } = subject;
    Ok(Some(
        Box::pin(build_local_status_response_inner(
            db,
            config,
            viewer,
            &local_status,
            &local_account,
            in_reply_to_account_id,
            media,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            poll_preload,
            viewer_state_preload,
            application_preload,
            include_quote,
        ))
        .await?,
    ))
}

async fn build_remote_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    remote_status: RemoteStatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    if !matches!(remote_status.visibility.as_str(), "public" | "unlisted") {
        return Ok(None);
    }

    let Some(actor) = find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await? else {
        return Ok(None);
    };

    Ok(Some(
        Box::pin(build_remote_status_response_inner(
            db,
            config,
            viewer,
            &remote_status,
            &actor,
            filter_matcher,
            counts_preload,
            None,
            None,
            None,
            None,
            None,
            include_quote,
        ))
        .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_status_quotes_count_sql_counts_local_and_remote_quotes_once() {
        let sql = accepted_status_quotes_count_sql();

        assert_eq!(sql.matches("quote_of_uri = ?1").count(), 2);
        assert!(sql.contains("FROM statuses"));
        assert!(sql.contains("FROM remote_statuses"));
        assert!(sql.contains("UNION ALL"));
    }

    #[test]
    fn local_status_edited_at_ignores_missing_or_creation_timestamp() {
        assert_eq!(
            local_status_edited_at_from_updated_at("2026-05-01T00:00:00Z", None),
            None
        );
        assert_eq!(
            local_status_edited_at_from_updated_at(
                "2026-05-01T00:00:00Z",
                Some("2026-05-01T00:00:00Z".to_owned())
            ),
            None
        );
    }

    #[test]
    fn local_status_edited_at_preserves_real_edit_timestamp() {
        assert_eq!(
            local_status_edited_at_from_updated_at(
                "2026-05-01T00:00:00Z",
                Some("2026-05-02T00:00:00Z".to_owned())
            ),
            Some("2026-05-02T00:00:00Z".to_owned())
        );
    }

    #[test]
    fn mention_lookup_keys_partition_local_and_remote_handles() {
        let handles = vec![
            AccountHandle {
                username: "Alice".to_owned(),
                domain: Some("social.example".to_owned()),
            },
            AccountHandle {
                username: "Bob".to_owned(),
                domain: Some("Remote.Example".to_owned()),
            },
            AccountHandle::local("Carol"),
        ];

        let keys = mention_lookup_keys(&handles, "social.example");

        assert_eq!(
            keys,
            MentionLookupKeys {
                local_usernames: vec!["alice".to_owned(), "carol".to_owned()],
                remote_pairs: vec![("bob".to_owned(), "remote.example".to_owned())],
            }
        );
    }

    #[test]
    fn mention_remote_pair_lowercases_username_and_domain() {
        let handle = AccountHandle {
            username: "Bob".to_owned(),
            domain: Some("Remote.Example".to_owned()),
        };

        assert_eq!(
            mention_remote_pair(&handle),
            Some(("bob".to_owned(), "remote.example".to_owned()))
        );
        assert_eq!(mention_remote_pair(&AccountHandle::local("alice")), None);
    }

    #[test]
    fn quote_state_uses_placeholder_for_terminal_states() {
        assert!(quote_state_uses_placeholder("revoked"));
        assert!(quote_state_uses_placeholder("rejected"));
        assert!(quote_state_uses_placeholder("unauthorized"));
        assert!(quote_state_uses_placeholder("deleted"));
        assert!(!quote_state_uses_placeholder("pending"));
        assert!(!quote_state_uses_placeholder("accepted"));
    }

    #[test]
    fn remote_media_attachment_values_allows_empty_attachments() {
        assert!(remote_media_attachment_values(&[]).is_empty());
    }

    #[test]
    fn remote_quote_visibility_is_embeddable_for_public_timelines() {
        assert!(remote_quote_visibility_is_embeddable("public"));
        assert!(remote_quote_visibility_is_embeddable("unlisted"));
        assert!(!remote_quote_visibility_is_embeddable("private"));
        assert!(!remote_quote_visibility_is_embeddable("direct"));
    }

    #[test]
    fn accepted_quote_document_state_matches_mastodon_state_name() {
        assert_eq!(accepted_quote_document_state(), "accepted");
    }

    #[test]
    fn unauthorized_quote_document_uses_placeholder_shape() {
        let document = unauthorized_quote_document();

        assert_eq!(document["state"], serde_json::json!("unauthorized"));
        assert_eq!(document["quoted_status"], serde_json::Value::Null);
    }

    #[test]
    fn preloaded_quote_count_returns_zero_for_known_absent_uri() {
        let preload = StatusQuoteCountsPreload {
            counts: HashMap::new(),
            preloaded_uris: HashSet::from(["https://example.test/statuses/1".to_owned()]),
        };

        assert_eq!(preload.count("https://example.test/statuses/1"), Some(0));
        assert_eq!(preload.count("https://example.test/statuses/2"), None);
    }

    #[test]
    fn preloaded_local_status_response_viewer_state_maps_flags_without_db_lookup() {
        let viewer = local_account_fixture();
        let status = status_row_fixture(
            "status-1",
            Some("https://social.example/users/alice/statuses/status-1"),
        );
        let target_uri = local_status_target_uri(&status);
        let preload = LocalStatusViewerStatePreload {
            favourited_target_uris: HashSet::from([target_uri.clone()]),
            reblogged_target_uris: HashSet::new(),
            bookmarked_target_uris: HashSet::from([target_uri]),
            pinned_status_ids: HashSet::from(["status-1".to_owned()]),
            has_thread_mutes: false,
        };

        let state =
            preloaded_local_status_response_viewer_state(Some(&viewer), &status, Some(&preload));

        assert_eq!(
            state,
            Some(LocalStatusPreloadedViewerState {
                favourited: true,
                reblogged: false,
                bookmarked: true,
                pinned: true,
                muted: Some(false),
            })
        );
    }

    #[test]
    fn preloaded_local_status_response_viewer_state_defers_mute_when_thread_mutes_exist() {
        let viewer = local_account_fixture();
        let status = status_row_fixture(
            "status-1",
            Some("https://social.example/users/alice/statuses/status-1"),
        );
        let preload = LocalStatusViewerStatePreload {
            favourited_target_uris: HashSet::new(),
            reblogged_target_uris: HashSet::new(),
            bookmarked_target_uris: HashSet::new(),
            pinned_status_ids: HashSet::new(),
            has_thread_mutes: true,
        };

        let state =
            preloaded_local_status_response_viewer_state(Some(&viewer), &status, Some(&preload));

        assert_eq!(
            state,
            Some(LocalStatusPreloadedViewerState {
                muted: None,
                ..LocalStatusPreloadedViewerState::default()
            })
        );
    }

    #[test]
    fn preloaded_remote_status_response_viewer_state_maps_flags_without_db_lookup() {
        let viewer = local_account_fixture();
        let preload = RemoteStatusViewerStatePreload {
            favourited_status_ids: HashSet::from(["status-1".to_owned()]),
            reblogged_status_ids: HashSet::new(),
            bookmarked_status_ids: HashSet::from(["status-1".to_owned()]),
            muted_actor_uris: HashSet::from(["https://remote.example/users/alice".to_owned()]),
        };

        let state = preloaded_remote_status_response_viewer_state(
            Some(&viewer),
            "status-1",
            "https://remote.example/users/alice",
            Some(&preload),
        );

        assert_eq!(
            state,
            Some(RemoteStatusResponseViewerState {
                favourited: true,
                reblogged: false,
                bookmarked: true,
                muted: true,
            })
        );
    }

    #[test]
    fn preloaded_remote_status_response_viewer_state_defaults_for_anonymous_viewer() {
        let preload = RemoteStatusViewerStatePreload {
            favourited_status_ids: HashSet::from(["status-1".to_owned()]),
            reblogged_status_ids: HashSet::from(["status-1".to_owned()]),
            bookmarked_status_ids: HashSet::from(["status-1".to_owned()]),
            muted_actor_uris: HashSet::from(["https://remote.example/users/alice".to_owned()]),
        };

        let state = preloaded_remote_status_response_viewer_state(
            None,
            "status-1",
            "https://remote.example/users/alice",
            Some(&preload),
        );

        assert_eq!(state, Some(RemoteStatusResponseViewerState::default()));
    }

    #[test]
    fn remote_reblog_wrapper_response_overlays_wrapper_fields_and_clears_embedded_body() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let wrapper_actor = remote_actor_row_fixture();
        let wrapper_status =
            remote_status_row_fixture("wrapper-status", "https://remote.example/announce/1");
        let embedded_status =
            remote_status_row_fixture("embedded-status", "https://remote.example/statuses/1");
        let mut embedded =
            MastodonStatusResponse::from_remote_row(&embedded_status, &wrapper_actor, &config);
        embedded.media_attachments = vec![serde_json::json!({"id": "media-1"})];
        embedded.quote = Some(serde_json::json!({"state": "accepted"}));

        let response = remote_reblog_wrapper_response_from_embedded(
            Some(embedded),
            &wrapper_status,
            &wrapper_actor,
            &config,
        );

        assert_eq!(response.id, "wrapper-status");
        assert_eq!(response.uri, "https://remote.example/announce/1");
        assert!(response.reblog.is_some());
        assert!(response.content.is_empty());
        assert!(response.media_attachments.is_empty());
        assert!(response.quote.is_none());
    }

    #[test]
    fn local_reblog_wrapper_response_overlays_wrapper_fields_and_clears_embedded_body() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let wrapper_account = local_account_fixture();
        let wrapper_status = status_row_fixture(
            "wrapper-status",
            Some("https://social.example/users/alice/statuses/wrapper"),
        );
        let embedded_status = status_row_fixture(
            "embedded-status",
            Some("https://social.example/users/alice/statuses/embedded"),
        );
        let mut embedded = MastodonStatusResponse::from_row(
            &embedded_status,
            &wrapper_account,
            &config,
            None,
            Vec::new(),
        );
        embedded.media_attachments = vec![serde_json::json!({"id": "media-1"})];
        embedded.quote = Some(serde_json::json!({"state": "accepted"}));

        let response = local_reblog_wrapper_response_from_embedded(
            Some(embedded),
            &wrapper_status,
            &wrapper_account,
            Some("reply-account".to_owned()),
            &config,
        );

        assert_eq!(response.id, "wrapper-status");
        assert_eq!(
            response.uri,
            "https://social.example/users/alice/statuses/wrapper"
        );
        assert_eq!(
            response.in_reply_to_account_id.as_deref(),
            Some("reply-account")
        );
        assert!(response.reblog.is_some());
        assert!(response.content.is_empty());
        assert!(response.media_attachments.is_empty());
        assert!(response.quote.is_none());
    }

    fn remote_status_row_fixture(id: &str, object_uri: &str) -> RemoteStatusRow {
        RemoteStatusRow {
            id: id.to_owned(),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: object_uri.to_owned(),
            url: None,
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 0,
            language: Some("en".to_owned()),
            quote_state: "accepted".to_owned(),
            published_at: "2026-05-10T01:02:03Z".to_owned(),
        }
    }

    fn remote_actor_row_fixture() -> RemoteActorRow {
        RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: "2026-05-01T00:00:00Z".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: "Alice".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@alice".to_owned()),
            avatar_url: None,
            header_url: None,
        }
    }

    fn status_row_fixture(id: &str, ap_id: Option<&str>) -> StatusRow {
        StatusRow {
            id: id.to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: ap_id.map(str::to_owned),
            in_reply_to_id: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            _text_content: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 0,
            language: Some("en".to_owned()),
            quote_approval_policy: None,
            quote_state: "accepted".to_owned(),
            application_id: None,
            created_at: "2026-05-10T01:02:03Z".to_owned(),
            updated_at: None,
        }
    }

    fn local_account_fixture() -> LocalAccount {
        LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: Vec::new(),
            locked: false,
            bot: false,
            discoverable: true,
            default_post_visibility: "public".to_owned(),
            default_quote_policy: "public".to_owned(),
            default_sensitive: false,
            default_language: Some("en".to_owned()),
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-05-01T00:00:00Z".to_owned(),
        }
    }
}

async fn build_local_reblog_wrapper_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    wrapper_status: &StatusRow,
    wrapper_account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    let embedded = build_reblog_embedded_response(
        db,
        config,
        viewer,
        boost_of_uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        include_quote,
    )
    .await?;

    Ok(local_reblog_wrapper_response_from_embedded(
        embedded,
        wrapper_status,
        wrapper_account,
        in_reply_to_account_id,
        config,
    ))
}
