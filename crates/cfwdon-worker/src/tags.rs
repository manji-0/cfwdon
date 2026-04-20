use std::cmp::Reverse;
use std::collections::HashSet;

use crate::content_helpers::{
    extract_hashtags_from_html, extract_hashtags_from_text, tag_history_stub, tag_rest_id, tag_url,
};
use crate::responses::{MastodonTagHistoryEntry, MastodonTagResponse};
use crate::{
    ResolvedTimelineCursor, list_local_public_timeline_statuses,
    list_remote_public_timeline_statuses,
};
use cfwdon_core::AppConfig;
use serde::Deserialize;
use url::Url;
use worker::d1::D1Type;
use worker::{D1Database, Result};

pub(crate) fn tag_search_rank(query: &str, tag: &str) -> (u8, String) {
    (
        crate::search_text_match_rank(query, tag),
        normalize_hashtag(tag),
    )
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub(crate) struct TagSearchMetrics {
    pub(crate) statuses_count: u64,
    pub(crate) accounts_count: u64,
    pub(crate) last_status_at: Option<String>,
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
    value.trim().trim_start_matches('#').to_ascii_lowercase()
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
        ["tags", tag] => *tag,
        ["explore", "tags", tag] => *tag,
        _ => return None,
    };
    let normalized = normalize_hashtag(tag);
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
    let mut matches = Vec::new();
    let mut seen = HashSet::new();

    let cursor = ResolvedTimelineCursor::default();
    for status in list_local_public_timeline_statuses(db, &cursor, fetch_limit).await? {
        for tag in extract_hashtags_from_text(&status._text_content) {
            if tag.contains(&needle) && seen.insert(tag.clone()) {
                matches.push(tag);
            }
        }
    }

    for (status, _) in list_remote_public_timeline_statuses(db, &cursor, fetch_limit).await? {
        for tag in extract_hashtags_from_html(&status.content_html) {
            if tag.contains(&needle) && seen.insert(tag.clone()) {
                matches.push(tag);
            }
        }
    }

    let mut ranked_matches = Vec::with_capacity(matches.len());
    for tag in matches {
        ranked_matches.push((tag.clone(), load_tag_search_metrics(db, &tag).await?));
    }

    let matches = paginate_tag_search_matches(&needle, ranked_matches, limit, offset);

    let mut responses = Vec::with_capacity(matches.len());
    for (tag, metrics) in matches {
        responses.push(build_tag_response_with_metrics(config, &tag, metrics));
    }

    Ok(responses)
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
    let local = load_local_tag_search_metrics(db, tag).await?;
    let remote = load_remote_tag_search_metrics(db, tag).await?;
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

async fn load_local_tag_search_metrics(db: &D1Database, tag: &str) -> Result<TagSearchMetrics> {
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

async fn load_remote_tag_search_metrics(db: &D1Database, tag: &str) -> Result<TagSearchMetrics> {
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
