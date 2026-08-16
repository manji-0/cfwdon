//! Batch preloads for status API responses.
//!
//! Application, quote-count, and viewer-state lookups are shared by timeline
//! and detail builders so a page pays one query set instead of one per status.

use super::{
    AppConfig, LocalAccount, MastodonPollResponsePreload, RemoteActorRow, RemoteStatusRow,
    StatusRow, account_has_thread_mutes, config_with_resolved_custom_emojis, count_rows,
    find_oauth_app_by_id, find_oauth_apps_by_ids, find_statuses_by_ap_ids, find_statuses_by_ids,
    load_mastodon_poll_response, load_status_updated_at, local_status_identity_from_uri,
    local_status_ids_thread_muted_by, local_status_target_uri,
};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use worker::{Result, d1::D1Type};

use crate::{D1Database, json_string_array, sql_in_json_each};

/// Resolves the custom emoji registry a status response needs.
///
/// Batch callers (timelines) resolve the registry once per request and pass it
/// as `resolved_config`; single-status callers pass `None` and pay for one
/// lookup. Without this, every status in a page re-reads `custom_emojis`.
pub(crate) async fn status_response_config<'a>(
    db: &D1Database,
    config: &'a AppConfig,
    resolved_config: Option<&'a AppConfig>,
) -> Result<Cow<'a, AppConfig>> {
    match resolved_config {
        Some(resolved) => Ok(Cow::Borrowed(resolved)),
        None => Ok(Cow::Owned(
            config_with_resolved_custom_emojis(db, config).await?,
        )),
    }
}

pub(crate) async fn build_status_application(
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
    requested_application_ids: HashSet<i64>,
    applications: HashMap<i64, serde_json::Value>,
}

impl StatusApplicationPreload {
    pub(crate) fn application(
        &self,
        application_id: Option<i64>,
    ) -> Option<Option<serde_json::Value>> {
        let Some(application_id) = application_id else {
            return Some(None);
        };
        if self.requested_application_ids.contains(&application_id) {
            return Some(self.applications.get(&application_id).cloned());
        }

        None
    }
}

fn collect_status_application_id(
    application_ids: &mut Vec<i64>,
    seen_application_ids: &mut HashSet<i64>,
    status: &StatusRow,
) {
    if let Some(application_id) = status.application_id
        && seen_application_ids.insert(application_id)
    {
        application_ids.push(application_id);
    }
}

fn collect_local_reblog_target_refs(
    config: &AppConfig,
    status_ids: &mut Vec<String>,
    ap_ids: &mut Vec<String>,
    status: &StatusRow,
) {
    let Some(boost_of_uri) = status.boost_of_uri.as_deref() else {
        return;
    };
    if let Some((_, status_id)) = local_status_identity_from_uri(config, boost_of_uri) {
        status_ids.push(status_id);
    } else {
        ap_ids.push(boost_of_uri.to_owned());
    }
}

pub(crate) async fn preload_status_applications(
    db: &D1Database,
    config: &AppConfig,
    statuses: &[&StatusRow],
) -> Result<StatusApplicationPreload> {
    let mut application_ids = Vec::new();
    let mut seen_application_ids = HashSet::new();
    let mut reblog_target_status_ids = Vec::new();
    let mut reblog_target_ap_ids = Vec::new();
    for status in statuses {
        collect_status_application_id(&mut application_ids, &mut seen_application_ids, status);
        collect_local_reblog_target_refs(
            config,
            &mut reblog_target_status_ids,
            &mut reblog_target_ap_ids,
            status,
        );
    }

    let (reblog_targets_by_id, reblog_targets_by_ap_id) = futures_util::try_join!(
        find_statuses_by_ids(db, &reblog_target_status_ids),
        find_statuses_by_ap_ids(db, &reblog_target_ap_ids),
    )?;
    for status in reblog_targets_by_id
        .iter()
        .chain(reblog_targets_by_ap_id.iter())
    {
        collect_status_application_id(&mut application_ids, &mut seen_application_ids, status);
    }

    let applications = find_oauth_apps_by_ids(db, &application_ids)
        .await?
        .into_iter()
        .map(|app| (app.id, status_application_value(app.name, app.website)))
        .collect();

    Ok(StatusApplicationPreload {
        requested_application_ids: seen_application_ids,
        applications,
    })
}

pub(crate) async fn local_status_edited_at(
    db: &D1Database,
    status: &StatusRow,
) -> Result<Option<String>> {
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

fn accepted_status_quotes_count_sql() -> &'static str {
    // Read from the pre-aggregated quote_target_counts table (maintained by
    // triggers in migration 114) for a single indexed lookup.
    "SELECT COALESCE(quotes_count, 0) AS count
     FROM quote_target_counts
     WHERE target_uri = ?1"
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

    pub(crate) fn extend(&mut self, other: Self) {
        self.counts.extend(other.counts);
        self.preloaded_uris.extend(other.preloaded_uris);
    }
}

#[derive(Debug, Default)]
pub(crate) struct LocalStatusViewerStatePreload {
    favourited_target_uris: HashSet<String>,
    reblogged_target_uris: HashSet<String>,
    bookmarked_target_uris: HashSet<String>,
    pinned_status_ids: HashSet<String>,
    muted_status_ids: HashSet<String>,
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

    fn muted(&self, status_id: &str) -> bool {
        self.has_thread_mutes && self.muted_status_ids.contains(status_id)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct LocalStatusPreloadedViewerState {
    pub(crate) favourited: bool,
    pub(crate) reblogged: bool,
    pub(crate) bookmarked: bool,
    pub(crate) pinned: bool,
    pub(crate) muted: Option<bool>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct LocalStatusResponseViewerState {
    pub(crate) favourited: bool,
    pub(crate) reblogged: bool,
    pub(crate) bookmarked: bool,
    pub(crate) pinned: bool,
    pub(crate) muted: bool,
}

pub(crate) fn preloaded_local_status_response_viewer_state(
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
            muted: Some(preload.muted(&status.id)),
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
pub(crate) struct RemoteStatusResponseViewerState {
    pub(crate) favourited: bool,
    pub(crate) reblogged: bool,
    pub(crate) bookmarked: bool,
    pub(crate) muted: bool,
}

pub(crate) fn preloaded_remote_status_response_viewer_state(
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

    let uris_json = json_string_array(target_uris);
    let sql = format!(
        "SELECT target_uri
         FROM {table}
         WHERE account_id = ?1
           AND target_uri {}",
        sql_in_json_each(2)
    );
    let bindings = [D1Type::Text(account_id), D1Type::Text(uris_json.as_str())];
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(crate::d1_results::<TargetUriRow>(&result)?
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

    let ids_json = json_string_array(status_ids);
    let sql = format!(
        "SELECT remote_status_id
         FROM {table}
         WHERE account_id = ?1
           AND remote_status_id {}",
        sql_in_json_each(2)
    );
    let bindings = [D1Type::Text(account_id), D1Type::Text(ids_json.as_str())];
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(crate::d1_results::<RemoteStatusIdRow>(&result)?
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

    let ids_json = json_string_array(status_ids);
    let sql = format!(
        "SELECT status_id
         FROM status_pins
         WHERE account_id = ?1
           AND status_id {}",
        sql_in_json_each(2)
    );
    let bindings = [D1Type::Text(account_id), D1Type::Text(ids_json.as_str())];
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(crate::d1_results::<StatusIdRow>(&result)?
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
    let muted_status_ids = if has_thread_mutes {
        local_status_ids_thread_muted_by(db, account_id, statuses).await?
    } else {
        HashSet::new()
    };

    Ok(LocalStatusViewerStatePreload {
        favourited_target_uris,
        reblogged_target_uris,
        bookmarked_target_uris,
        pinned_status_ids,
        muted_status_ids,
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

    let uris_json = json_string_array(&uris);
    let sql = format!(
        "SELECT target_uri AS quote_of_uri, quotes_count AS count
         FROM quote_target_counts
         WHERE target_uri {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(uris_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;
    let counts = crate::d1_results::<StatusQuoteCountRow>(&result)?
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

pub(crate) async fn local_status_poll_response(
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

pub(crate) async fn status_quotes_count(
    db: &D1Database,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    status_uri: &str,
) -> Result<u64> {
    if let Some(count) = quote_counts_preload.and_then(|counts| counts.count(status_uri)) {
        return Ok(count);
    }

    count_status_quotes_by_uri(db, status_uri).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::LocalAccountRecord;

    #[test]
    fn accepted_status_quotes_count_sql_reads_from_quote_target_counts() {
        let sql = accepted_status_quotes_count_sql();

        assert!(sql.contains("quote_target_counts"));
        assert!(sql.contains("target_uri = ?1"));
        assert!(!sql.contains("UNION ALL"));
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
    fn preloaded_quote_count_returns_zero_for_known_absent_uri() {
        let preload = StatusQuoteCountsPreload {
            counts: HashMap::new(),
            preloaded_uris: HashSet::from(["https://example.test/statuses/1".to_owned()]),
        };

        assert_eq!(preload.count("https://example.test/statuses/1"), Some(0));
        assert_eq!(preload.count("https://example.test/statuses/2"), None);
    }

    #[test]
    fn preloaded_status_application_distinguishes_absent_from_unknown() {
        let application = serde_json::json!({
            "name": "cfwdon",
            "website": "https://apps.example/cfwdon",
        });
        let preload = StatusApplicationPreload {
            requested_application_ids: HashSet::from([1, 2]),
            applications: HashMap::from([(1, application.clone())]),
        };

        assert_eq!(preload.application(None), Some(None));
        assert_eq!(preload.application(Some(1)), Some(Some(application)));
        assert_eq!(preload.application(Some(2)), Some(None));
        assert_eq!(preload.application(Some(3)), None);
    }

    #[test]
    fn collect_status_application_id_deduplicates_ids() {
        let mut status = status_row_fixture("status-1", None);
        status.application_id = Some(7);
        let mut application_ids = Vec::new();
        let mut seen_application_ids = HashSet::new();

        collect_status_application_id(&mut application_ids, &mut seen_application_ids, &status);
        collect_status_application_id(&mut application_ids, &mut seen_application_ids, &status);

        assert_eq!(application_ids, vec![7]);
    }

    #[test]
    fn collect_local_reblog_target_refs_splits_local_urls_from_ap_ids() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let mut local_reblog = status_row_fixture("reblog-1", None);
        local_reblog.boost_of_uri =
            Some("https://social.example/users/Alice/statuses/status-1".to_owned());
        let mut remote_reblog = status_row_fixture("reblog-2", None);
        remote_reblog.boost_of_uri = Some("https://remote.example/statuses/status-2".to_owned());
        let mut status_ids = Vec::new();
        let mut ap_ids = Vec::new();

        collect_local_reblog_target_refs(&config, &mut status_ids, &mut ap_ids, &local_reblog);
        collect_local_reblog_target_refs(&config, &mut status_ids, &mut ap_ids, &remote_reblog);

        assert_eq!(status_ids, vec!["status-1".to_owned()]);
        assert_eq!(
            ap_ids,
            vec!["https://remote.example/statuses/status-2".to_owned()]
        );
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
            muted_status_ids: HashSet::new(),
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
    fn preloaded_local_status_response_viewer_state_resolves_mute_from_preloaded_ids() {
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
            muted_status_ids: HashSet::from(["status-1".to_owned()]),
            has_thread_mutes: true,
        };

        let state =
            preloaded_local_status_response_viewer_state(Some(&viewer), &status, Some(&preload));

        assert_eq!(
            state,
            Some(LocalStatusPreloadedViewerState {
                muted: Some(true),
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

    fn status_row_fixture(id: &str, ap_id: Option<&str>) -> StatusRow {
        StatusRow {
            id: id.to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: ap_id.map(str::to_owned),
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            text: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: cfwdon_domain::Visibility::Public,
            sensitive: false,
            language: Some("en".to_owned()),
            quote_approval_policy: None,
            quote_state: cfwdon_domain::QuoteState::Accepted,
            application_id: None,
            card_json: None,
            created_at: "2026-05-10T01:02:03Z".to_owned(),
            updated_at: None,
        }
    }

    fn local_account_fixture() -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", "alice"))
    }
}
