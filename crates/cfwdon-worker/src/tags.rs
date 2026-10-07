use crate::db_utils::{d1_results, json_string_array, sql_in_json_each};
use crate::response::MastodonTagResponse;
use crate::responses::MastodonTagHistoryEntry;
use crate::search::normalize_search_match_text;
use crate::statuses::{list_local_public_timeline_statuses, list_remote_public_timeline_statuses};
use crate::time_html::now_unix_timestamp;
use crate::timelines::ResolvedTimelineCursor;
use crate::tracked_d1::D1Database;
use crate::trends_cache::{TRENDING_TAGS_CACHE_SIZE, store_trending_tags_cache};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::content_helpers::{
    extract_hashtags_from_html, extract_hashtags_from_text, tag_rest_id, tag_url,
};
use crate::search::search_text_match_rank;
use cfwdon_core::AppConfig;
use serde::Deserialize;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;
use worker::Result;
use worker::d1::D1Type;
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
    normalize_search_match_text(value.trim().trim_start_matches('#'))
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
    // Stored tags are only lowercased, so a prefix range misses accent-folded
    // matches (`munchen` for `#München`). When the range leaves room, recent
    // public posts supply such tags, counted from the hashtag tables.
    if matches.len() < fetch_limit as usize {
        let known = matches
            .iter()
            .map(|(tag, _)| tag.clone())
            .collect::<HashSet<_>>();
        let folded = scan_recent_folded_tags_for_v2(db, &needle, fetch_limit, &known).await?;
        matches.extend(load_tag_search_metrics_for_tags(db, &folded).await?);
    }
    let tags = paginate_tag_search_matches(&needle, matches, limit, offset)
        .into_iter()
        .map(|(tag, _)| tag)
        .collect();
    build_tag_responses(db, config, tags).await
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
    query_tag_search_metrics(
        db,
        &tag_search_metrics_sql("h.tag >= ?1 AND h.tag < ?2", 3),
        &bindings,
    )
    .await
}

async fn load_tag_search_metrics_for_tags(
    db: &D1Database,
    tags: &[String],
) -> Result<Vec<(String, TagSearchMetrics)>> {
    if tags.is_empty() {
        return Ok(Vec::new());
    }
    let tags_json = json_string_array(tags);
    let bindings = [
        D1Type::Text(tags_json.as_str()),
        D1Type::Integer(tags.len() as i32),
    ];
    query_tag_search_metrics(
        db,
        &tag_search_metrics_sql(&format!("h.tag {}", sql_in_json_each(1)), 2),
        &bindings,
    )
    .await
}

/// Per-tag usage over public local and remote statuses, from the hashtag
/// tables only.
fn tag_search_metrics_sql(tag_predicate: &str, limit_slot: usize) -> String {
    format!(
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
             CROSS JOIN statuses s ON s.id = h.status_id
             WHERE s.visibility = 'public'
               AND {tag_predicate}
             GROUP BY h.tag
             UNION ALL
             SELECT h.tag AS tag,
                    COUNT(*) AS statuses_count,
                    COUNT(DISTINCT h.actor_uri) AS accounts_count,
                    MAX(substr(h.published_at, 1, 10)) AS last_status_at
             FROM remote_status_hashtags h
             CROSS JOIN remote_statuses rs ON rs.id = h.status_id
             WHERE rs.visibility = 'public'
               AND {tag_predicate}
             GROUP BY h.tag
         )
         GROUP BY tag
         ORDER BY statuses_count DESC, last_status_at DESC, tag ASC
         LIMIT ?{limit_slot}"
    )
}

async fn query_tag_search_metrics(
    db: &D1Database,
    sql: &str,
    bindings: &[D1Type<'_>],
) -> Result<Vec<(String, TagSearchMetrics)>> {
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;
    Ok(d1_results::<TagSearchRow>(&result)?
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
        .collect())
}

/// Tags in recent public posts that match `needle` after accent folding and
/// are not in `known`.
async fn scan_recent_folded_tags_for_v2(
    db: &D1Database,
    needle: &str,
    fetch_limit: u32,
    known: &HashSet<String>,
) -> Result<Vec<String>> {
    let cursor = ResolvedTimelineCursor::default();
    let (local_statuses, remote_statuses) = futures_util::try_join!(
        list_local_public_timeline_statuses(db, &cursor, fetch_limit),
        list_remote_public_timeline_statuses(db, &cursor, fetch_limit),
    )?;
    let mut seen = HashSet::new();
    Ok(local_statuses
        .iter()
        .flat_map(|status| extract_hashtags_from_text(&status.text))
        .chain(
            remote_statuses
                .iter()
                .flat_map(|(status, _)| extract_hashtags_from_html(&status.content_html)),
        )
        .filter(|tag| {
            tag_matches_search_query(needle, tag)
                && !known.contains(tag)
                && seen.insert(tag.clone())
        })
        .collect())
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

    let mut statements = Vec::new();
    for tag in existing.difference(&next) {
        let bindings = [D1Type::Text(status_id), D1Type::Text(tag.as_str())];
        statements.push(
            db.prepare("DELETE FROM status_hashtags WHERE status_id = ?1 AND tag = ?2")
                .bind_refs(bindings.iter())?,
        );
    }

    for tag in next.difference(&existing) {
        let bindings = [
            D1Type::Text(status_id),
            D1Type::Text(tag.as_str()),
            D1Type::Text(account_id),
            D1Type::Text(created_at),
        ];
        statements.push(
            db.prepare(
                "INSERT OR IGNORE INTO status_hashtags (status_id, tag, account_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind_refs(bindings.iter())?,
        );
    }

    if !statements.is_empty() {
        db.batch(statements).await?;
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

    Ok(d1_results::<IndexedTagRow>(&result)?
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

    let mut statements = Vec::new();
    for tag in existing.difference(&next) {
        let bindings = [D1Type::Text(status_id), D1Type::Text(tag.as_str())];
        statements.push(
            db.prepare("DELETE FROM remote_status_hashtags WHERE status_id = ?1 AND tag = ?2")
                .bind_refs(bindings.iter())?,
        );
    }
    for tag in next.difference(&existing) {
        let bindings = [
            D1Type::Text(status_id),
            D1Type::Text(tag.as_str()),
            D1Type::Text(actor_uri),
            D1Type::Text(published_at),
        ];
        statements.push(
            db.prepare(
                "INSERT OR IGNORE INTO remote_status_hashtags (status_id, tag, actor_uri, published_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind_refs(bindings.iter())?,
        );
    }

    if !statements.is_empty() {
        db.batch(statements).await?;
    }
    Ok(())
}

pub(crate) async fn load_remote_status_hashtag_names(
    db: &D1Database,
    status_id: &str,
) -> Result<HashSet<String>> {
    let status_binding = D1Type::Text(status_id);
    let result = db
        .prepare("SELECT tag FROM remote_status_hashtags WHERE status_id = ?1")
        .bind_refs(&status_binding)?
        .all()
        .await?;

    Ok(d1_results::<IndexedTagRow>(&result)?
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
    let mut history = load_tag_daily_history(db, std::slice::from_ref(&tag)).await?;
    Ok(build_tag_response_with_history(
        config,
        &tag,
        history.remove(&tag).unwrap_or_default(),
    ))
}

fn build_tag_response_with_history(
    config: &AppConfig,
    tag: &str,
    daily: HashMap<String, (u64, u64)>,
) -> MastodonTagResponse {
    MastodonTagResponse {
        id: tag_rest_id(tag),
        name: tag.to_owned(),
        url: tag_url(config, tag),
        history: tag_history_entries(now_unix_timestamp(), &daily),
        following: None,
        featuring: None,
    }
}

/// Days of history Mastodon reports for a tag.
const TAG_HISTORY_DAYS: i64 = 7;
const SECONDS_PER_DAY: i64 = 86_400;

/// Mastodon's tag history: one entry per UTC day for the last week, newest
/// first, with `day` as the day's start in UNIX seconds and zero-filled gaps.
/// `daily` maps `YYYY-MM-DD` to (uses, accounts).
fn tag_history_entries(
    now: i64,
    daily: &HashMap<String, (u64, u64)>,
) -> Vec<MastodonTagHistoryEntry> {
    let today = now.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY;
    (0..TAG_HISTORY_DAYS)
        .map(|offset| {
            let day_start = today - offset * SECONDS_PER_DAY;
            let date = time::OffsetDateTime::from_unix_timestamp(day_start)
                .map(|value| value.date().to_string())
                .unwrap_or_default();
            let (uses, accounts) = daily.get(&date).copied().unwrap_or_default();
            MastodonTagHistoryEntry {
                day: day_start.to_string(),
                uses: uses.to_string(),
                accounts: accounts.to_string(),
            }
        })
        .collect()
}

/// Per-day public uses and distinct authors of each tag over the last week,
/// in one query for the whole page of tags.
async fn load_tag_daily_history(
    db: &D1Database,
    tags: &[String],
) -> Result<HashMap<String, HashMap<String, (u64, u64)>>> {
    if tags.is_empty() {
        return Ok(HashMap::new());
    }
    #[derive(Deserialize)]
    struct DailyRow {
        tag: String,
        day: String,
        uses: u64,
        accounts: u64,
    }
    let cutoff = time::OffsetDateTime::from_unix_timestamp(
        now_unix_timestamp() - (TAG_HISTORY_DAYS - 1) * SECONDS_PER_DAY,
    )
    .map(|value| value.date().to_string())
    .unwrap_or_default();
    let tags_json = json_string_array(tags);
    let bindings = [
        D1Type::Text(tags_json.as_str()),
        D1Type::Text(cutoff.as_str()),
    ];
    let in_tags = sql_in_json_each(1);
    let result = db
        .prepare(format!(
            "SELECT tag, day, SUM(uses) AS uses, SUM(accounts) AS accounts
             FROM (
                 SELECT h.tag AS tag, substr(h.created_at, 1, 10) AS day,
                        COUNT(*) AS uses, COUNT(DISTINCT h.account_id) AS accounts
                 FROM status_hashtags h
                 CROSS JOIN statuses s ON s.id = h.status_id
                 WHERE h.tag {in_tags}
                   AND h.created_at >= ?2
                   AND s.visibility = 'public'
                 GROUP BY h.tag, day
                 UNION ALL
                 SELECT h.tag AS tag, substr(h.published_at, 1, 10) AS day,
                        COUNT(*) AS uses, COUNT(DISTINCT h.actor_uri) AS accounts
                 FROM remote_status_hashtags h
                 CROSS JOIN remote_statuses rs ON rs.id = h.status_id
                 WHERE h.tag {in_tags}
                   AND h.published_at >= ?2
                   AND rs.visibility = 'public'
                 GROUP BY h.tag, day
             )
             GROUP BY tag, day"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    let mut history = HashMap::<String, HashMap<String, (u64, u64)>>::new();
    for row in d1_results::<DailyRow>(&result)? {
        history
            .entry(row.tag)
            .or_default()
            .insert(row.day, (row.uses, row.accounts));
    }
    Ok(history)
}

async fn build_tag_responses(
    db: &D1Database,
    config: &AppConfig,
    tags: Vec<String>,
) -> Result<Vec<MastodonTagResponse>> {
    let mut history = load_tag_daily_history(db, &tags).await?;
    Ok(tags
        .into_iter()
        .map(|tag| {
            let daily = history.remove(&tag).unwrap_or_default();
            build_tag_response_with_history(config, &tag, daily)
        })
        .collect())
}

const TRENDING_TAGS_WINDOW_DAYS: i64 = 3;
const TRENDING_TAGS_MIN_USES: u64 = 10;

fn trending_tags_cutoff_iso() -> Result<String> {
    let now = OffsetDateTime::from_unix_timestamp(now_unix_timestamp()).map_err(|error| {
        worker::Error::RustError(format!("invalid current unix timestamp: {error}"))
    })?;
    (now - Duration::days(TRENDING_TAGS_WINDOW_DAYS))
        .format(&Rfc3339)
        .map_err(|error| {
            worker::Error::RustError(format!("failed to format trending tags cutoff: {error}"))
        })
}

fn trending_tags_metrics_sql() -> &'static str {
    "SELECT tag,
            SUM(statuses_count) AS statuses_count,
            SUM(accounts_count) AS accounts_count,
            MAX(last_status_at) AS last_status_at
     FROM (
         SELECT h.tag AS tag,
                COUNT(*) AS statuses_count,
                COUNT(DISTINCT h.account_id) AS accounts_count,
                MAX(h.created_at) AS last_status_at
         FROM status_hashtags h
         JOIN statuses s ON s.id = h.status_id
         WHERE s.visibility = 'public'
           AND h.created_at >= ?1
         GROUP BY h.tag
         UNION ALL
         SELECT h.tag AS tag,
                COUNT(*) AS statuses_count,
                COUNT(DISTINCT h.actor_uri) AS accounts_count,
                MAX(h.published_at) AS last_status_at
         FROM remote_status_hashtags h
         JOIN remote_statuses rs ON rs.id = h.status_id
         WHERE rs.visibility = 'public'
           AND h.published_at >= ?1
         GROUP BY h.tag
     )
     GROUP BY tag
     HAVING SUM(statuses_count) >= ?2
     ORDER BY statuses_count DESC,
              accounts_count DESC,
              last_status_at DESC,
              tag ASC
     LIMIT ?3"
}

pub(crate) async fn list_trending_tag_metrics(
    db: &D1Database,
    fetch_limit: u32,
) -> Result<Vec<(String, TagSearchMetrics)>> {
    let cutoff = trending_tags_cutoff_iso()?;
    let bindings = [
        D1Type::Text(cutoff.as_str()),
        D1Type::Integer(i32::try_from(TRENDING_TAGS_MIN_USES).unwrap_or(i32::MAX)),
        D1Type::Integer(i32::try_from(fetch_limit).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(trending_tags_metrics_sql())
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(d1_results::<TagSearchRow>(&result)?
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
        .collect())
}

pub(crate) async fn trending_tags_documents(
    db: &D1Database,
    config: &AppConfig,
    offset: u32,
    limit: u32,
) -> Result<Vec<MastodonTagResponse>> {
    let fetch_limit = offset
        .saturating_add(limit)
        .clamp(limit, TRENDING_TAGS_CACHE_SIZE);
    let tags = list_trending_tag_metrics(db, fetch_limit)
        .await?
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|(tag, _)| tag)
        .collect();
    build_tag_responses(db, config, tags).await
}

pub(crate) async fn refresh_trending_tags_cache(db: &D1Database, config: &AppConfig) -> Result<()> {
    let documents = trending_tags_documents(db, config, 0, TRENDING_TAGS_CACHE_SIZE).await?;
    let values = documents
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    store_trending_tags_cache(&values).await
}

#[cfg(test)]
mod trending_tags_tests {
    use super::trending_tags_metrics_sql;

    #[test]
    fn trending_tags_metrics_sql_uses_hashtag_tables() {
        let sql = trending_tags_metrics_sql();
        assert!(sql.contains("FROM status_hashtags"));
        assert!(sql.contains("FROM remote_status_hashtags"));
        assert!(sql.contains("h.created_at >= ?1"));
        assert!(sql.contains("h.published_at >= ?1"));
        assert!(sql.contains("HAVING SUM(statuses_count) >= ?2"));
        assert!(sql.contains("ORDER BY statuses_count DESC"));
        assert!(!sql.contains("text_content"));
    }
}

#[cfg(test)]
mod unit_tests;

#[cfg(test)]
mod history_tests {
    use super::tag_history_entries;
    use std::collections::HashMap;

    #[test]
    fn tag_history_lists_seven_zero_filled_days_newest_first() {
        // 2026-10-06T12:00:00Z
        let now = 1_791_288_000;
        let daily = HashMap::from([
            ("2026-10-06".to_owned(), (3, 2)),
            ("2026-10-04".to_owned(), (1, 1)),
        ]);
        let history = tag_history_entries(now, &daily);
        assert_eq!(history.len(), 7);
        assert_eq!(history[0].day, "1791244800");
        assert_eq!(
            (history[0].uses.as_str(), history[0].accounts.as_str()),
            ("3", "2")
        );
        assert_eq!(history[1].uses, "0");
        assert_eq!(history[2].uses, "1");
        assert_eq!(history[6].day, (1_791_244_800 - 6 * 86_400).to_string());
    }
}
