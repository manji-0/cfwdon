use std::collections::HashSet;

use crate::content_helpers::{
    extract_hashtags_from_html, extract_hashtags_from_text, tag_history_stub, tag_rest_id, tag_url,
};
use crate::db_utils::count_rows_like;
use crate::responses::{MastodonTagHistoryEntry, MastodonTagResponse};
use crate::{
    ResolvedTimelineCursor, list_local_public_timeline_statuses,
    list_remote_public_timeline_statuses,
};
use cfwdon_core::AppConfig;
use url::Url;
use worker::{D1Database, Result};

pub(crate) fn tag_search_rank(query: &str, tag: &str) -> (u8, String) {
    (
        crate::search_text_match_rank(query, tag),
        normalize_hashtag(tag),
    )
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
) -> Result<Vec<MastodonTagResponse>> {
    let needle = normalize_hashtag(query);
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    let mut seen = HashSet::new();

    let cursor = ResolvedTimelineCursor::default();
    for status in list_local_public_timeline_statuses(db, &cursor, 200).await? {
        for tag in extract_hashtags_from_text(&status._text_content) {
            if tag.contains(&needle) && seen.insert(tag.clone()) {
                matches.push(tag);
            }
        }
    }

    for (status, _) in list_remote_public_timeline_statuses(db, &cursor, 200).await? {
        for tag in extract_hashtags_from_html(&status.content_html) {
            if tag.contains(&needle) && seen.insert(tag.clone()) {
                matches.push(tag);
            }
        }
    }

    matches.sort_by_key(|tag| tag_search_rank(&needle, tag));
    matches.truncate(limit as usize);

    let mut responses = Vec::with_capacity(matches.len());
    for tag in matches {
        responses.push(build_tag_response(db, config, &tag).await?);
    }

    Ok(responses)
}

pub(crate) async fn build_tag_response(
    db: &D1Database,
    config: &AppConfig,
    tag: &str,
) -> Result<MastodonTagResponse> {
    let tag = normalize_hashtag(tag);
    let local_count = count_local_public_statuses_by_tag(db, &tag).await?;
    let remote_count = count_remote_public_statuses_by_tag(db, &tag).await?;
    let total_uses = local_count + remote_count;
    let accounts = count_local_accounts_for_tag(db, &tag).await?
        + count_remote_accounts_for_tag(db, &tag).await?;

    Ok(MastodonTagResponse {
        id: tag_rest_id(&tag),
        name: tag.clone(),
        url: tag_url(config, &tag),
        history: if total_uses == 0 {
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
                uses: total_uses.to_string(),
                accounts: accounts.to_string(),
            }]
        },
        following: false,
        featured: false,
    })
}

async fn count_local_public_statuses_by_tag(db: &D1Database, tag: &str) -> Result<u64> {
    count_rows_like(
        db,
        "SELECT COUNT(*) AS count
         FROM statuses
         WHERE visibility = 'public'
           AND lower(text_content) LIKE ?1",
        &format!("%#{}%", normalize_hashtag(tag)),
    )
    .await
}

async fn count_remote_public_statuses_by_tag(db: &D1Database, tag: &str) -> Result<u64> {
    count_rows_like(
        db,
        "SELECT COUNT(*) AS count
         FROM remote_statuses
         WHERE visibility = 'public'
           AND lower(content_html) LIKE ?1",
        &format!("%#{}%", normalize_hashtag(tag)),
    )
    .await
}

async fn count_local_accounts_for_tag(db: &D1Database, tag: &str) -> Result<u64> {
    count_rows_like(
        db,
        "SELECT COUNT(DISTINCT account_id) AS count
         FROM statuses
         WHERE visibility = 'public'
           AND lower(text_content) LIKE ?1",
        &format!("%#{}%", normalize_hashtag(tag)),
    )
    .await
}

async fn count_remote_accounts_for_tag(db: &D1Database, tag: &str) -> Result<u64> {
    count_rows_like(
        db,
        "SELECT COUNT(DISTINCT actor_uri) AS count
         FROM remote_statuses
         WHERE visibility = 'public'
           AND lower(content_html) LIKE ?1",
        &format!("%#{}%", normalize_hashtag(tag)),
    )
    .await
}
