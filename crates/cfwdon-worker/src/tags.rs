use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::content_helpers::{
    extract_hashtags_from_html, extract_hashtags_from_text, tag_history_stub, tag_rest_id, tag_url,
};
use crate::responses::{MastodonTagHistoryEntry, MastodonTagResponse};
use crate::search::search_text_match_rank;
use crate::statuses::{
    ResolvedTimelineCursor, list_local_public_timeline_statuses,
    list_remote_public_timeline_statuses,
};
use cfwdon_core::AppConfig;
use serde::Deserialize;
use url::Url;
use worker::d1::D1Type;
use worker::{D1Database, Result};

pub(crate) fn tag_search_rank(query: &str, tag: &str) -> (u8, String) {
    (search_text_match_rank(query, tag), normalize_hashtag(tag))
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub(crate) struct TagSearchMetrics {
    pub(crate) statuses_count: u64,
    pub(crate) accounts_count: u64,
    pub(crate) last_status_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TagSearchRow {
    tag: String,
    statuses_count: u64,
    accounts_count: u64,
    last_status_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IndexedTagRow {
    tag: String,
}

pub(crate) fn tag_search_sort_key(
    query: &str,
    tag: &str,
    statuses_count: u64,
    last_status_at: Option<&str>,
) -> (u8, u64, Reverse<Option<String>>, String) {
    let (match_rank, normalized) = tag_search_rank(query, tag);
    (
        match_rank,
        u64::MAX - statuses_count,
        Reverse(last_status_at.map(ToOwned::to_owned)),
        normalized,
    )
}

pub(crate) fn paginate_tag_search_matches(
    query: &str,
    mut matches: Vec<(String, TagSearchMetrics)>,
    limit: u32,
    offset: u32,
) -> Vec<(String, TagSearchMetrics)> {
    matches.sort_by_key(|(tag, metrics)| {
        tag_search_sort_key(
            query,
            tag,
            metrics.statuses_count,
            metrics.last_status_at.as_deref(),
        )
    });
    matches
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect()
}

pub(crate) fn normalize_hashtag(value: &str) -> String {
    crate::normalize_search_match_text(value.trim().trim_start_matches('#'))
}

pub(crate) fn tag_matches_search_query(query: &str, tag: &str) -> bool {
    let query = normalize_hashtag(query);
    let tag = normalize_hashtag(tag);
    !query.is_empty() && tag.starts_with(&query)
}

pub(crate) async fn resolve_search_tag(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
) -> Result<Option<MastodonTagResponse>> {
    let Some(tag) = resolve_search_tag_name(query) else {
        return Ok(None);
    };

    Ok(Some(build_tag_response(db, config, &tag).await?))
}

pub(crate) fn resolve_search_tag_name(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    if query.starts_with('#') {
        let tag = normalize_hashtag(query);
        return (!tag.is_empty()).then_some(tag);
    }

    if let Ok(url) = Url::parse(query) {
        return search_tag_name_from_path(url.path());
    }

    if query.starts_with('/') {
        return search_tag_name_from_path(query);
    }

    None
}

pub(crate) fn search_tag_name_from_path(path: &str) -> Option<String> {
    let segments = path
        .split('?')
        .next()
        .unwrap_or(path)
        .split('#')
        .next()
        .unwrap_or(path)
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let tag = match segments.as_slice() {
        [prefix, tag] if prefix.eq_ignore_ascii_case("tags") => *tag,
        [explore, prefix, tag]
            if explore.eq_ignore_ascii_case("explore") && prefix.eq_ignore_ascii_case("tags") =>
        {
            *tag
        }
        _ => return None,
    };
    let normalized = normalize_hashtag(
        &urlencoding::decode(tag)
            .map(|value| value.into_owned())
            .unwrap_or_else(|_| tag.to_owned()),
    );
    (!normalized.is_empty()).then_some(normalized)
}

pub(crate) async fn search_tags_for_v2(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<MastodonTagResponse>> {
    let needle = resolve_search_tag_name(query).unwrap_or_else(|| normalize_hashtag(query));
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let fetch_limit = limit.saturating_add(offset).clamp(limit, 200);
    let mut matches = search_indexed_tags_for_v2(db, &needle, fetch_limit).await?;
    let mut merged = matches.drain(..).collect::<HashMap<_, _>>();
    for (tag, metrics) in scan_tags_for_v2(db, &needle, fetch_limit).await? {
        merged.insert(tag, metrics);
    }

    Ok(
        paginate_tag_search_matches(&needle, merged.into_iter().collect(), limit, offset)
            .into_iter()
            .map(|(tag, metrics)| build_tag_response_with_metrics(config, &tag, metrics))
            .collect(),
    )
}

async fn search_indexed_tags_for_v2(
    db: &D1Database,
    needle: &str,
    fetch_limit: u32,
) -> Result<Vec<(String, TagSearchMetrics)>> {
    let upper_bound =
        tag_prefix_upper_bound(needle).unwrap_or_else(|| format!("{needle}\u{10ffff}"));
    let bindings = [
        D1Type::Text(needle),
        D1Type::Text(upper_bound.as_str()),
        D1Type::Integer(fetch_limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT tag,
                    SUM(statuses_count) AS statuses_count,
                    SUM(accounts_count) AS accounts_count,
                    MAX(last_status_at) AS last_status_at
             FROM (
                 SELECT h.tag AS tag,
                        COUNT(*) AS statuses_count,
                        COUNT(DISTINCT h.account_id) AS accounts_count,
                        MAX(substr(h.created_at, 1, 10)) AS last_status_at
	                 FROM status_hashtags h
	                 JOIN statuses s ON s.id = h.status_id
	                 WHERE s.visibility = 'public'
	                   AND h.tag >= ?1
	                   AND h.tag < ?2
	                 GROUP BY h.tag
	                 UNION ALL
	                 SELECT h.tag AS tag,
                        COUNT(*) AS statuses_count,
                        COUNT(DISTINCT h.actor_uri) AS accounts_count,
                        MAX(substr(h.published_at, 1, 10)) AS last_status_at
	                 FROM remote_status_hashtags h
	                 JOIN remote_statuses rs ON rs.id = h.status_id
	                 WHERE rs.visibility = 'public'
	                   AND h.tag >= ?1
	                   AND h.tag < ?2
	                 GROUP BY h.tag
	             )
	             GROUP BY tag
	             ORDER BY statuses_count DESC, last_status_at DESC, tag ASC
	             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<TagSearchRow>()?
        .into_iter()
        .map(|row| {
            (
                row.tag,
                TagSearchMetrics {
                    statuses_count: row.statuses_count,
                    accounts_count: row.accounts_count,
                    last_status_at: row.last_status_at,
                },
            )
        })
        .collect::<Vec<_>>())
}

async fn scan_tags_for_v2(
    db: &D1Database,
    needle: &str,
    fetch_limit: u32,
) -> Result<Vec<(String, TagSearchMetrics)>> {
    let mut matches = Vec::new();
    let mut seen = HashSet::new();

    let cursor = ResolvedTimelineCursor::default();
    for status in list_local_public_timeline_statuses(db, &cursor, fetch_limit).await? {
        for tag in extract_hashtags_from_text(&status.text) {
            if tag_matches_search_query(needle, &tag) && seen.insert(tag.clone()) {
                matches.push(tag);
            }
        }
    }

    for (status, _) in list_remote_public_timeline_statuses(db, &cursor, fetch_limit).await? {
        for tag in extract_hashtags_from_html(&status.content_html) {
            if tag_matches_search_query(needle, &tag) && seen.insert(tag.clone()) {
                matches.push(tag);
            }
        }
    }

    let mut ranked_matches = Vec::with_capacity(matches.len());
    for tag in matches {
        ranked_matches.push((
            tag.clone(),
            load_scanned_tag_search_metrics(db, &tag).await?,
        ));
    }

    Ok(ranked_matches)
}

fn tag_prefix_upper_bound(value: &str) -> Option<String> {
    let (last_index, last_char) = value.char_indices().next_back()?;
    let next_char = char::from_u32(last_char as u32 + 1)?;
    let mut upper = value[..last_index].to_owned();
    upper.push(next_char);
    Some(upper)
}

pub(crate) async fn replace_local_status_hashtags(
    db: &D1Database,
    status_id: &str,
    account_id: &str,
    created_at: &str,
    text: &str,
) -> Result<()> {
    let existing = load_local_status_hashtag_names(db, status_id).await?;
    let next = extract_hashtags_from_text(text)
        .into_iter()
        .collect::<HashSet<_>>();

    for tag in existing.difference(&next) {
        let bindings = [D1Type::Text(status_id), D1Type::Text(tag.as_str())];
        db.prepare("DELETE FROM status_hashtags WHERE status_id = ?1 AND tag = ?2")
            .bind_refs(bindings.iter())?
            .run()
            .await?;
    }

    for tag in next.difference(&existing) {
        let bindings = [
            D1Type::Text(status_id),
            D1Type::Text(tag.as_str()),
            D1Type::Text(account_id),
            D1Type::Text(created_at),
        ];
        db.prepare(
            "INSERT OR IGNORE INTO status_hashtags (status_id, tag, account_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

async fn load_local_status_hashtag_names(
    db: &D1Database,
    status_id: &str,
) -> Result<HashSet<String>> {
    let status_binding = D1Type::Text(status_id);
    let result = db
        .prepare("SELECT tag FROM status_hashtags WHERE status_id = ?1")
        .bind_refs(&status_binding)?
        .all()
        .await?;

    Ok(result
        .results::<IndexedTagRow>()?
        .into_iter()
        .map(|row| row.tag)
        .collect())
}

pub(crate) async fn replace_remote_status_hashtags(
    db: &D1Database,
    status_id: &str,
    actor_uri: &str,
    published_at: &str,
    content_html: &str,
) -> Result<()> {
    let existing = load_remote_status_hashtag_names(db, status_id).await?;
    let next = extract_hashtags_from_html(content_html)
        .into_iter()
        .collect::<HashSet<_>>();

    for tag in existing.difference(&next) {
        let bindings = [D1Type::Text(status_id), D1Type::Text(tag.as_str())];
        db.prepare("DELETE FROM remote_status_hashtags WHERE status_id = ?1 AND tag = ?2")
            .bind_refs(bindings.iter())?
            .run()
            .await?;
    }

    for tag in next.difference(&existing) {
        let bindings = [
            D1Type::Text(status_id),
            D1Type::Text(tag.as_str()),
            D1Type::Text(actor_uri),
            D1Type::Text(published_at),
        ];
        db.prepare(
            "INSERT OR IGNORE INTO remote_status_hashtags (status_id, tag, actor_uri, published_at)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

async fn load_remote_status_hashtag_names(
    db: &D1Database,
    status_id: &str,
) -> Result<HashSet<String>> {
    let status_binding = D1Type::Text(status_id);
    let result = db
        .prepare("SELECT tag FROM remote_status_hashtags WHERE status_id = ?1")
        .bind_refs(&status_binding)?
        .all()
        .await?;

    Ok(result
        .results::<IndexedTagRow>()?
        .into_iter()
        .map(|row| row.tag)
        .collect())
}

pub(crate) async fn build_tag_response(
    db: &D1Database,
    config: &AppConfig,
    tag: &str,
) -> Result<MastodonTagResponse> {
    let tag = normalize_hashtag(tag);
    Ok(build_tag_response_with_metrics(
        config,
        &tag,
        load_tag_search_metrics(db, &tag).await?,
    ))
}

fn build_tag_response_with_metrics(
    config: &AppConfig,
    tag: &str,
    metrics: TagSearchMetrics,
) -> MastodonTagResponse {
    MastodonTagResponse {
        id: tag_rest_id(tag),
        name: tag.to_owned(),
        url: tag_url(config, tag),
        history: if metrics.statuses_count == 0 {
            tag_history_stub()
        } else {
            vec![MastodonTagHistoryEntry {
                day: js_sys::Date::new_0()
                    .to_iso_string()
                    .as_string()
                    .unwrap_or_default()
                    .chars()
                    .take(10)
                    .collect(),
                uses: metrics.statuses_count.to_string(),
                accounts: metrics.accounts_count.to_string(),
            }]
        },
        following: false,
        featured: false,
    }
}

pub(crate) async fn load_tag_search_metrics(
    db: &D1Database,
    tag: &str,
) -> Result<TagSearchMetrics> {
    let scanned = load_scanned_tag_search_metrics(db, tag).await?;
    if scanned.statuses_count > 0 {
        return Ok(scanned);
    }
    load_indexed_tag_search_metrics(db, tag).await
}

async fn load_scanned_tag_search_metrics(db: &D1Database, tag: &str) -> Result<TagSearchMetrics> {
    let local = load_scanned_local_tag_search_metrics(db, tag).await?;
    let remote = load_scanned_remote_tag_search_metrics(db, tag).await?;
    Ok(TagSearchMetrics {
        statuses_count: local.statuses_count + remote.statuses_count,
        accounts_count: local.accounts_count + remote.accounts_count,
        last_status_at: match (local.last_status_at, remote.last_status_at) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        },
    })
}

async fn load_indexed_tag_search_metrics(db: &D1Database, tag: &str) -> Result<TagSearchMetrics> {
    let tag = normalize_hashtag(tag);
    let bindings = [D1Type::Text(tag.as_str())];
    Ok(db
        .prepare(
            "SELECT COALESCE(SUM(statuses_count), 0) AS statuses_count,
                    COALESCE(SUM(accounts_count), 0) AS accounts_count,
                    MAX(last_status_at) AS last_status_at
             FROM (
                 SELECT COUNT(*) AS statuses_count,
                        COUNT(DISTINCT h.account_id) AS accounts_count,
                        MAX(substr(h.created_at, 1, 10)) AS last_status_at
                 FROM status_hashtags h
                 JOIN statuses s ON s.id = h.status_id
                 WHERE s.visibility = 'public'
                   AND h.tag = ?1
                 UNION ALL
                 SELECT COUNT(*) AS statuses_count,
                        COUNT(DISTINCT h.actor_uri) AS accounts_count,
                        MAX(substr(h.published_at, 1, 10)) AS last_status_at
                 FROM remote_status_hashtags h
                 JOIN remote_statuses rs ON rs.id = h.status_id
                 WHERE rs.visibility = 'public'
                   AND h.tag = ?1
             )",
        )
        .bind_refs(bindings.iter())?
        .first::<TagSearchMetrics>(None)
        .await?
        .unwrap_or_default())
}

async fn load_scanned_local_tag_search_metrics(
    db: &D1Database,
    tag: &str,
) -> Result<TagSearchMetrics> {
    let pattern = format!("%#{}%", normalize_hashtag(tag));
    let bindings = [D1Type::Text(pattern.as_str())];
    Ok(db
        .prepare(
            "SELECT COUNT(*) AS statuses_count,
                    COUNT(DISTINCT account_id) AS accounts_count,
                    MAX(substr(created_at, 1, 10)) AS last_status_at
             FROM statuses
             WHERE visibility = 'public'
               AND lower(text_content) LIKE ?1",
        )
        .bind_refs(bindings.iter())?
        .first::<TagSearchMetrics>(None)
        .await?
        .unwrap_or_default())
}

async fn load_scanned_remote_tag_search_metrics(
    db: &D1Database,
    tag: &str,
) -> Result<TagSearchMetrics> {
    let pattern = format!("%#{}%", normalize_hashtag(tag));
    let bindings = [D1Type::Text(pattern.as_str())];
    Ok(db
        .prepare(
            "SELECT COUNT(*) AS statuses_count,
                    COUNT(DISTINCT actor_uri) AS accounts_count,
                    MAX(substr(published_at, 1, 10)) AS last_status_at
             FROM remote_statuses
             WHERE visibility = 'public'
               AND lower(content_html) LIKE ?1",
        )
        .bind_refs(bindings.iter())?
        .first::<TagSearchMetrics>(None)
        .await?
        .unwrap_or_default())
}
