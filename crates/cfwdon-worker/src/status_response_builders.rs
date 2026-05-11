use crate::{
    AccountFilterMatcher, AppConfig, LocalAccount, MastodonPollResponsePreload,
    MastodonStatusResponse, MediaAttachmentRow, RemoteActorRow, RemoteMastodonPollResponsePreload,
    RemoteStatusAttachmentRow, RemoteStatusEditUpdatedAtPreload, RemoteStatusRow,
    StatusCountsPreload, StatusRow, account_has_thread_mutes, actor_url,
    build_remote_status_card_value, build_status_card_value, can_view_local_status, count_rows,
    effective_remote_status_quote_state, effective_status_quote_state, find_account_by_id,
    find_local_status_by_object_uri, find_media_attachments_by_status_id, find_oauth_app_by_id,
    find_remote_actor_by_actor_uri, find_remote_status_attachments_by_status_id,
    find_remote_status_by_url_or_object_uri, has_remote_status_edit_snapshots, is_blocking_actor,
    is_local_follower_authorized, is_local_status_bookmarked_by, is_local_status_favourited_by,
    is_local_status_pinned_by, is_local_status_reblogged_by, is_local_status_thread_muted_by,
    is_muted_actor, is_remote_status_bookmarked_by, is_remote_status_favourited_by,
    is_remote_status_reblogged_by, load_in_reply_to_account_id, load_local_status_counts,
    load_mastodon_poll_response, load_remote_mastodon_poll_response, load_remote_status_counts,
    load_remote_status_updated_at, load_status_filtered, load_status_updated_at,
    local_status_target_uri, strip_html_tags,
};
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
    let mut mentions = Vec::new();

    for handle in crate::extract_account_handles_from_text(text, config) {
        if handle.is_local_to(&config.instance_domain) {
            let Some(account) = crate::find_account_by_username(db, &handle.username).await? else {
                continue;
            };
            mentions.push(serde_json::json!({
                "id": account.id,
                "username": account.username,
                "url": actor_url(config, &account.username),
                "acct": account.acct(),
            }));
            continue;
        }

        let Some(domain) = handle.domain.as_deref() else {
            continue;
        };
        let Some(actor) =
            crate::find_remote_actor_by_username_domain(db, &handle.username, domain).await?
        else {
            continue;
        };
        mentions.push(serde_json::json!({
            "id": crate::remote_account_rest_id(&actor.actor_uri),
            "username": actor.username,
            "url": actor.profile_url.clone().unwrap_or_else(|| actor.actor_uri.clone()),
            "acct": format!("{}@{}", actor.username, actor.domain),
        }));
    }

    Ok(mentions)
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
        None,
        None,
        None,
        true,
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
    response.application = build_status_application(db, status.application_id).await?;
    response.card = build_status_card_value(&status._text_content);
    response.poll = local_status_poll_response(db, poll_preload, &status.id, viewer).await?;
    response.mentions = build_status_mentions(db, config, &status._text_content).await?;
    let (favourites_count, reblogs_count) =
        local_status_counts(db, counts_preload, &status.id).await?;
    response.favourites_count = favourites_count;
    response.favourited = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) => preload.favourited(status),
        (Some(viewer), None) => is_local_status_favourited_by(db, &viewer.id, status).await?,
        (None, _) => false,
    };
    response.reblogs_count = reblogs_count;
    response.quotes_count = status_quotes_count(db, quote_counts_preload, &response.uri).await?;
    response.reblogged = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) => preload.reblogged(status),
        (Some(viewer), None) => is_local_status_reblogged_by(db, &viewer.id, status).await?,
        (None, _) => false,
    };
    response.bookmarked = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) => preload.bookmarked(status),
        (Some(viewer), None) => is_local_status_bookmarked_by(db, &viewer.id, status).await?,
        (None, _) => false,
    };
    response.pinned = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) => preload.pinned(&status.id),
        (Some(viewer), None) => is_local_status_pinned_by(db, &viewer.id, &status.id).await?,
        (None, _) => false,
    };
    response.muted = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) if preload.can_skip_thread_mute_lookup() => false,
        (Some(viewer), _) => is_local_status_thread_muted_by(db, &viewer.id, status).await?,
        (None, _) => false,
    };
    let updated_at = match status.updated_at.as_deref() {
        Some(updated_at) => Some(updated_at.to_owned()),
        None => load_status_updated_at(db, &status.id).await?,
    };
    response.edited_at = match updated_at {
        Some(updated_at) if updated_at != status.created_at => Some(updated_at),
        _ => None,
    };
    response.filtered = match viewer {
        Some(viewer) => {
            filtered_status_for_viewer(
                db,
                filter_matcher,
                &viewer.id,
                &status.id,
                &status._text_content,
                &status.spoiler_text,
            )
            .await?
        }
        None => Vec::new(),
    };
    response.quote_approval = Some(build_local_quote_approval(db, status, viewer, account).await?);
    if include_quote {
        response.quote = build_quoted_status_value(
            db,
            config,
            viewer,
            status.quote_of_uri.as_deref(),
            Some(effective_status_quote_state(status)),
            true,
            filter_matcher,
            counts_preload,
        )
        .await?;
    }
    Ok(response)
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
    let text_content = strip_html_tags(&status.content_html);
    let remote_attachments = match remote_attachments {
        Some(attachments) => attachments,
        None => find_remote_status_attachments_by_status_id(db, &status.id).await?,
    };
    response.card = build_remote_status_card_value(&text_content, &remote_attachments);
    response.media_attachments = remote_media_attachment_values(&remote_attachments);
    response.mentions = build_status_mentions(db, config, &text_content).await?;
    let (favourites_count, reblogs_count) =
        remote_status_counts(db, counts_preload, &status.id).await?;
    response.favourites_count = favourites_count;
    response.favourited = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) => preload.favourited(&status.id),
        (Some(viewer), None) => is_remote_status_favourited_by(db, &viewer.id, &status.id).await?,
        (None, _) => false,
    };
    response.reblogs_count = reblogs_count;
    response.quotes_count = status_quotes_count(db, quote_counts_preload, &response.uri).await?;
    response.reblogged = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) => preload.reblogged(&status.id),
        (Some(viewer), None) => is_remote_status_reblogged_by(db, &viewer.id, &status.id).await?,
        (None, _) => false,
    };
    response.bookmarked = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) => preload.bookmarked(&status.id),
        (Some(viewer), None) => is_remote_status_bookmarked_by(db, &viewer.id, &status.id).await?,
        (None, _) => false,
    };
    response.muted = match (viewer, viewer_state_preload) {
        (Some(_), Some(preload)) => preload.muted(&actor.actor_uri),
        (Some(viewer), None) => is_muted_actor(db, &viewer.id, &actor.actor_uri).await?,
        (None, _) => false,
    };
    response.poll = match poll_preload {
        Some(preload) => preload.poll_response(&status.id),
        None => load_remote_mastodon_poll_response(db, status, viewer).await?,
    };
    response.edited_at = match edit_updated_at_preload {
        Some(preload) => preload.updated_at(&status.id).map(ToOwned::to_owned),
        None => {
            if has_remote_status_edit_snapshots(db, &status.id).await? {
                load_remote_status_updated_at(db, &status.id).await?
            } else {
                None
            }
        }
    };
    response.filtered = match viewer {
        Some(viewer) => {
            filtered_status_for_viewer(
                db,
                filter_matcher,
                &viewer.id,
                &status.id,
                &text_content,
                &status.spoiler_text,
            )
            .await?
        }
        None => Vec::new(),
    };
    response.quote_approval = Some(build_remote_quote_approval(status));
    if include_quote {
        response.quote = build_quoted_status_value(
            db,
            config,
            viewer,
            status.quote_of_uri.as_deref(),
            Some(effective_remote_status_quote_state(status)),
            false,
            filter_matcher,
            counts_preload,
        )
        .await?;
    }
    Ok(response)
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

    if let Some(local_status) = find_local_status_by_object_uri(db, config, quote_of_uri).await? {
        let Some(local_account) = find_account_by_id(db, &local_status.account_id).await? else {
            return Ok(None);
        };
        if !can_view_local_status(db, &local_status, viewer, &local_account).await? {
            return Ok(Some(unauthorized_quote_document()));
        }
        let media = find_media_attachments_by_status_id(db, &local_status.id).await?;
        let mut response = MastodonStatusResponse::from_row(
            &local_status,
            &local_account,
            config,
            load_in_reply_to_account_id(db, &local_status).await?,
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
        response.favourited = match viewer {
            Some(viewer) => is_local_status_favourited_by(db, &viewer.id, &local_status).await?,
            None => false,
        };
        response.reblogs_count = reblogs_count;
        response.reblogged = match viewer {
            Some(viewer) => is_local_status_reblogged_by(db, &viewer.id, &local_status).await?,
            None => false,
        };
        response.bookmarked = match viewer {
            Some(viewer) => is_local_status_bookmarked_by(db, &viewer.id, &local_status).await?,
            None => false,
        };
        response.pinned = match viewer {
            Some(viewer) => is_local_status_pinned_by(db, &viewer.id, &local_status.id).await?,
            None => false,
        };
        response.muted = match viewer {
            Some(viewer) => is_local_status_thread_muted_by(db, &viewer.id, &local_status).await?,
            None => false,
        };
        response.quote = None;
        let state = local_quoted_status_document_state(db, config, viewer, &local_account).await?;
        return Ok(Some(quote_document_from_response(state, response)));
    }

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
        response.favourited = match viewer {
            Some(viewer) => {
                is_remote_status_favourited_by(db, &viewer.id, &remote_status.id).await?
            }
            None => false,
        };
        response.reblogs_count = reblogs_count;
        response.reblogged = match viewer {
            Some(viewer) => {
                is_remote_status_reblogged_by(db, &viewer.id, &remote_status.id).await?
            }
            None => false,
        };
        response.bookmarked = match viewer {
            Some(viewer) => {
                is_remote_status_bookmarked_by(db, &viewer.id, &remote_status.id).await?
            }
            None => false,
        };
        response.muted = match viewer {
            Some(viewer) => is_muted_actor(db, &viewer.id, &actor.actor_uri).await?,
            None => false,
        };
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
    let embedded = if let Some(local_status) =
        find_local_status_by_object_uri(db, config, boost_of_uri).await?
    {
        if let Some(local_account) = find_account_by_id(db, &local_status.account_id).await? {
            if can_view_local_status(db, &local_status, viewer, &local_account).await? {
                let media = find_media_attachments_by_status_id(db, &local_status.id).await?;
                Some(
                    Box::pin(build_local_status_response_inner(
                        db,
                        config,
                        viewer,
                        &local_status,
                        &local_account,
                        load_in_reply_to_account_id(db, &local_status).await?,
                        media,
                        filter_matcher,
                        counts_preload,
                        None,
                        None,
                        None,
                        include_quote,
                    ))
                    .await?,
                )
            } else {
                None
            }
        } else {
            None
        }
    } else if let Some(remote_status) =
        find_remote_status_by_url_or_object_uri(db, boost_of_uri).await?
    {
        if !matches!(remote_status.visibility.as_str(), "public" | "unlisted") {
            None
        } else if let Some(actor) =
            find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await?
        {
            Some(
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
            )
        } else {
            None
        }
    } else {
        None
    };

    let mut response = embedded.clone().unwrap_or_else(|| {
        MastodonStatusResponse::from_remote_row(wrapper_status, wrapper_actor, config)
    });
    response.id = wrapper_status.id.clone();
    response.created_at = wrapper_status.published_at.clone();
    response.in_reply_to_id = wrapper_status.in_reply_to_uri.clone();
    response.in_reply_to_account_id = None;
    response.visibility = wrapper_status.visibility.clone();
    response.uri = wrapper_status.object_uri.clone();
    response.url = wrapper_status
        .url
        .clone()
        .unwrap_or_else(|| wrapper_status.object_uri.clone());
    response.account = crate::MastodonAccountResponse::from_remote_actor(wrapper_actor);
    response.reblog =
        embedded.map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null));
    response.content.clear();
    response.text = None;
    response.media_attachments.clear();
    response.mentions.clear();
    response.tags.clear();
    response.emojis.clear();
    response.card = None;
    response.poll = None;
    response.quote = None;
    Ok(response)
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
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    let embedded = if let Some(local_status) =
        find_local_status_by_object_uri(db, config, boost_of_uri).await?
    {
        if let Some(local_account) = find_account_by_id(db, &local_status.account_id).await? {
            if can_view_local_status(db, &local_status, viewer, &local_account).await? {
                let media = find_media_attachments_by_status_id(db, &local_status.id).await?;
                Some(
                    Box::pin(build_local_status_response_inner(
                        db,
                        config,
                        viewer,
                        &local_status,
                        &local_account,
                        load_in_reply_to_account_id(db, &local_status).await?,
                        media,
                        filter_matcher,
                        counts_preload,
                        quote_counts_preload,
                        poll_preload,
                        viewer_state_preload,
                        include_quote,
                    ))
                    .await?,
                )
            } else {
                None
            }
        } else {
            None
        }
    } else if let Some(remote_status) =
        find_remote_status_by_url_or_object_uri(db, boost_of_uri).await?
    {
        if !matches!(remote_status.visibility.as_str(), "public" | "unlisted") {
            None
        } else if let Some(actor) =
            find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await?
        {
            Some(
                build_remote_status_response_inner(
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
                )
                .await?,
            )
        } else {
            None
        }
    } else {
        None
    };

    let mut response = embedded.clone().unwrap_or_else(|| {
        MastodonStatusResponse::from_row(
            wrapper_status,
            wrapper_account,
            config,
            in_reply_to_account_id.clone(),
            Vec::new(),
        )
    });
    response.id = wrapper_status.id.clone();
    response.created_at = wrapper_status.created_at.clone();
    response.in_reply_to_id = wrapper_status.in_reply_to_id.clone();
    response.in_reply_to_account_id = in_reply_to_account_id;
    response.visibility = wrapper_status.visibility.clone();
    response.uri = wrapper_status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, &wrapper_account.username),
            wrapper_status.id
        )
    });
    response.url = response.uri.clone();
    response.account = crate::MastodonAccountResponse::from_account(wrapper_account, config);
    response.reblog = embedded
        .clone()
        .map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null));
    response.content.clear();
    response.text = None;
    response.media_attachments.clear();
    response.mentions.clear();
    response.tags.clear();
    response.emojis.clear();
    response.card = None;
    response.poll = None;
    response.quote = None;
    Ok(response)
}
