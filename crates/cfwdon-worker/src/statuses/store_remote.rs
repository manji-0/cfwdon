use super::{
    AccountStatusVisibilityScope, D1Database, RemoteActorRow, RemoteStatusRecord, RemoteStatusRow,
    ResolvedTimelineCursor, Result, json_string_array, normalize_hashtag,
    remote_status_from_record, remote_statuses_from_records, sql_in_json_each, unique_ordered_refs,
};
use crate::{
    append_remote_status_id_cursor_parts, append_resolved_timeline_cursor_bindings,
    format_with_clauses, seekable_resolved_timeline_cursor_predicates,
};
use std::collections::HashSet;
use worker::d1::D1Type;

const REMOTE_STATUS_WITH_ACTOR_SELECT: &str = "rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable";

#[derive(Clone, Copy, Debug)]
pub(crate) struct RemoteAccountStatusListOptions<'a> {
    pub(crate) max_id: Option<&'a str>,
    pub(crate) min_id: Option<&'a str>,
    pub(crate) limit: u32,
    pub(crate) visibility: AccountStatusVisibilityScope,
    pub(crate) only_media: bool,
    pub(crate) exclude_replies: bool,
    pub(crate) exclude_reblogs: bool,
    pub(crate) tagged: Option<&'a str>,
}

pub(crate) async fn list_remote_public_timeline_statuses(
    db: &D1Database,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let (sql, bindings) = remote_public_timeline_statuses_sql(cursor, limit);
    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

fn remote_public_timeline_statuses_sql<'a>(
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = Vec::new();
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("rs.published_at", "rs.id", &slots);
    let sql = format!(
        "SELECT
            {REMOTE_STATUS_WITH_ACTOR_SELECT}
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'{cursor_predicates}
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

pub(crate) async fn list_remote_home_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let (sql, bindings) = remote_home_timeline_statuses_sql(viewer_account_id, cursor, limit);
    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

fn remote_home_timeline_statuses_sql<'a>(
    viewer_account_id: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = vec![D1Type::Text(viewer_account_id)];
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("rs.published_at", "rs.id", &slots);
    let sql = format!(
        "SELECT
            {REMOTE_STATUS_WITH_ACTOR_SELECT}
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         JOIN follows f
           ON f.target_actor_uri = rs.actor_uri
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE rs.visibility IN ('public', 'unlisted', 'private'){cursor_predicates}
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

pub(crate) async fn find_remote_statuses_with_actors_by_ids(
    db: &D1Database,
    status_ids: &[String],
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let ids = unique_ordered_refs(status_ids);
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let ids_json = json_string_array(&ids);
    let sql = remote_statuses_with_actors_by_ids_sql();
    let binding = D1Type::Text(ids_json.as_str());
    query_remote_statuses_with_actor(db, &sql, &[binding]).await
}

fn remote_statuses_with_actors_by_ids_sql() -> String {
    format!(
        "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.id {}",
        sql_in_json_each(1)
    )
}

pub(crate) async fn list_remote_public_statuses_by_tag(
    db: &D1Database,
    tag: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    list_remote_public_statuses_by_tags(db, &[normalize_hashtag(tag)], cursor, limit).await
}

pub(crate) async fn list_remote_public_statuses_by_tags(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let (mut rows, tags) =
        list_remote_public_statuses_by_tags_indexed(db, tags, cursor, limit).await?;
    if rows.len() >= limit as usize {
        return Ok(rows);
    }
    let mut seen_ids = rows
        .iter()
        .map(|(status, _)| status.id.clone())
        .collect::<HashSet<_>>();
    for (status, actor) in
        list_remote_public_statuses_by_tags_legacy(db, &tags, cursor, limit).await?
    {
        if seen_ids.insert(status.id.clone()) {
            rows.push((status, actor));
        }
    }
    rows.sort_by(|(left_status, _), (right_status, _)| {
        right_status
            .published_at
            .cmp(&left_status.published_at)
            .then_with(|| right_status.id.cmp(&left_status.id))
    });
    rows.truncate(limit as usize);
    Ok(rows)
}

async fn list_remote_public_statuses_by_tags_indexed(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<(Vec<(RemoteStatusRow, RemoteActorRow)>, Vec<String>)> {
    let tags = normalized_unique_remote_status_tags(tags);
    if tags.is_empty() {
        return Ok((Vec::new(), tags));
    }

    let (sql, bindings) = remote_public_statuses_by_tags_indexed_sql(&tags, cursor, limit);

    Ok((
        query_remote_statuses_with_actor(db, &sql, &bindings).await?,
        tags,
    ))
}

fn normalized_unique_remote_status_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.iter()
        .map(|tag| normalize_hashtag(tag))
        .filter(|tag| !tag.is_empty() && seen.insert(tag.clone()))
        .collect()
}

fn remote_public_statuses_by_tags_indexed_sql<'a>(
    tags: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let tag_placeholders = (1..=tags.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut bindings = tags
        .iter()
        .map(|tag| D1Type::Text(tag.as_str()))
        .collect::<Vec<_>>();
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("rs.published_at", "rs.id", &slots);
    let sql = format!(
        "SELECT
            {REMOTE_STATUS_WITH_ACTOR_SELECT}
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND rs.id IN (
               SELECT h.status_id
               FROM remote_status_hashtags h
               WHERE h.tag IN ({tag_placeholders})
           ){cursor_predicates}
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

async fn list_remote_public_statuses_by_tags_legacy(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let patterns = remote_public_statuses_by_tags_legacy_patterns(tags);
    let (sql, bindings) = remote_public_statuses_by_tags_legacy_sql(&patterns, cursor, limit);

    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

fn remote_public_statuses_by_tags_legacy_patterns(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| format!("%#{}%", normalize_hashtag(tag)))
        .collect()
}

fn remote_public_statuses_by_tags_legacy_sql<'a>(
    patterns: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let match_clause = (1..=patterns.len())
        .map(|index| format!("lower(rs.content_html) LIKE ?{index}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut bindings = patterns
        .iter()
        .map(|pattern| D1Type::Text(pattern.as_str()))
        .collect::<Vec<_>>();
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("rs.published_at", "rs.id", &slots);
    let sql = format!(
        "SELECT
            {REMOTE_STATUS_WITH_ACTOR_SELECT}
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND ({match_clause}){cursor_predicates}
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

pub(crate) async fn list_remote_public_statuses_by_link(
    db: &D1Database,
    urls: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    if urls.is_empty() {
        return Ok(Vec::new());
    }

    let patterns = remote_public_statuses_by_link_patterns(urls);
    let (sql, bindings) = remote_public_statuses_by_link_sql(&patterns, cursor, limit);
    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

fn remote_public_statuses_by_link_patterns(urls: &[String]) -> Vec<String> {
    urls.iter().map(|url| format!("%{url}%")).collect()
}

fn remote_public_statuses_by_link_sql<'a>(
    patterns: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let match_clause = (1..=patterns.len())
        .map(|position| {
            format!(
                "(rs.content_html LIKE ?{position} OR rs.url LIKE ?{position} OR rs.object_uri LIKE ?{position})"
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut bindings = patterns
        .iter()
        .map(|pattern| D1Type::Text(pattern.as_str()))
        .collect::<Vec<_>>();
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("rs.published_at", "rs.id", &slots);
    let sql = format!(
        "SELECT
            {REMOTE_STATUS_WITH_ACTOR_SELECT}
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND ra.discoverable = 1
           AND ({match_clause}){cursor_predicates}
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

pub(crate) async fn list_remote_statuses_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
    options: RemoteAccountStatusListOptions<'_>,
) -> Result<Vec<RemoteStatusRow>> {
    let tagged_pattern = remote_actor_status_tagged_pattern(options.tagged);
    let query_bindings =
        remote_actor_statuses_by_actor_uri_bindings(actor_uri, options, tagged_pattern.as_deref());
    let filter_predicates =
        remote_actor_status_filter_predicates(options, query_bindings.tagged_binding);
    let sql = remote_actor_statuses_by_actor_uri_sql(
        &filter_predicates,
        &query_bindings.cursor_parts,
        query_bindings.limit_binding,
    );

    query_remote_status_rows(db, &sql, &query_bindings.bindings).await
}

struct RemoteActorStatusQueryBindings<'a> {
    bindings: Vec<D1Type<'a>>,
    cursor_parts: crate::StatusIdCursorParts,
    tagged_binding: Option<usize>,
    limit_binding: usize,
}

fn remote_actor_statuses_by_actor_uri_bindings<'a>(
    actor_uri: &'a str,
    options: RemoteAccountStatusListOptions<'a>,
    tagged_pattern: Option<&'a str>,
) -> RemoteActorStatusQueryBindings<'a> {
    let mut bindings = vec![D1Type::Text(actor_uri)];
    let cursor_parts = append_remote_status_id_cursor_parts(
        &mut bindings,
        "remote_statuses",
        options.max_id,
        options.min_id,
    );
    let tagged_binding = if let Some(pattern) = tagged_pattern {
        bindings.push(D1Type::Text(pattern));
        Some(bindings.len())
    } else {
        None
    };
    bindings.push(D1Type::Integer(options.limit as i32));
    let limit_binding = bindings.len();

    RemoteActorStatusQueryBindings {
        bindings,
        cursor_parts,
        tagged_binding,
        limit_binding,
    }
}

fn remote_actor_status_tagged_pattern(tagged: Option<&str>) -> Option<String> {
    tagged
        .map(|tag| tag.trim().to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .map(|tag| format!("%{tag}%"))
}

fn remote_actor_status_filter_predicates(
    options: RemoteAccountStatusListOptions<'_>,
    tagged_binding: Option<usize>,
) -> Vec<String> {
    let mut predicates = vec!["actor_uri = ?1".to_owned()];

    match options.visibility {
        AccountStatusVisibilityScope::All => {}
        AccountStatusVisibilityScope::Public => {
            predicates.push("visibility IN ('public', 'unlisted')".to_owned());
        }
        AccountStatusVisibilityScope::PublicUnlistedPrivate => {
            predicates.push("visibility IN ('public', 'unlisted', 'private')".to_owned());
        }
    }
    if options.exclude_reblogs {
        predicates.push("boost_of_uri IS NULL".to_owned());
    }
    if options.exclude_replies {
        predicates.push("in_reply_to_uri IS NULL".to_owned());
    }
    if options.only_media {
        predicates.push(
            "EXISTS (
                SELECT 1 FROM remote_status_attachments media
                WHERE media.status_id = remote_statuses.id
            )"
            .to_owned(),
        );
    }
    if let Some(tagged_binding) = tagged_binding {
        predicates.push(format!("lower(content_html) LIKE ?{tagged_binding}"));
    }

    predicates
}

fn remote_actor_statuses_by_actor_uri_sql(
    predicates: &[String],
    cursor_parts: &crate::StatusIdCursorParts,
    limit_binding: usize,
) -> String {
    let with_clause = format_with_clauses(&cursor_parts.with_clauses);
    let mut all_predicates = predicates.to_vec();
    all_predicates.extend(cursor_parts.predicates.clone());
    format!(
        "{with_clause}SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
         FROM remote_statuses
         WHERE {}
         ORDER BY published_at DESC, id DESC
         LIMIT ?{limit_binding}",
        all_predicates.join("\n           AND ")
    )
}

pub(crate) async fn list_public_remote_statuses_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
    max_id: Option<&str>,
    min_id: Option<&str>,
    limit: u32,
) -> Result<Vec<RemoteStatusRow>> {
    let (sql, bindings) = public_remote_statuses_by_actor_uri_sql(actor_uri, max_id, min_id, limit);
    query_remote_status_rows(db, &sql, &bindings).await
}

fn public_remote_statuses_by_actor_uri_sql<'a>(
    actor_uri: &'a str,
    max_id: Option<&'a str>,
    min_id: Option<&'a str>,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = vec![D1Type::Text(actor_uri)];
    let cursor_parts =
        append_remote_status_id_cursor_parts(&mut bindings, "remote_statuses", max_id, min_id);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_binding = bindings.len();
    let with_clause = format_with_clauses(&cursor_parts.with_clauses);
    let mut predicates = vec![
        "actor_uri = ?1".to_owned(),
        "visibility IN ('public', 'unlisted')".to_owned(),
    ];
    predicates.extend(cursor_parts.predicates);
    let sql = format!(
        "{with_clause}SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
         FROM remote_statuses
         WHERE {}
         ORDER BY published_at DESC, id DESC
         LIMIT ?{limit_binding}",
        predicates.join("\n               AND ")
    );
    (sql, bindings)
}

pub(crate) async fn list_direct_remote_replies_by_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    query_remote_statuses_with_actor(
        db,
        direct_remote_replies_by_uri_sql(),
        &direct_remote_replies_by_uri_bindings(object_uri),
    )
    .await
}

pub(crate) async fn list_remote_direct_statuses_mentioning_viewer(
    db: &D1Database,
    mention_pattern: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let (sql, bindings) =
        remote_direct_statuses_mentioning_viewer_sql(mention_pattern, cursor, limit);
    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

fn remote_direct_statuses_mentioning_viewer_sql<'a>(
    mention_pattern: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = vec![D1Type::Text(mention_pattern)];
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("rs.published_at", "rs.id", &slots);
    let sql = format!(
        "SELECT
            {REMOTE_STATUS_WITH_ACTOR_SELECT}
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'direct'
           AND (lower(rs.content_html) LIKE ?1 OR lower(rs.spoiler_text) LIKE ?1){cursor_predicates}
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

fn direct_remote_replies_by_uri_bindings(object_uri: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(object_uri)]
}

fn direct_remote_replies_by_uri_sql() -> &'static str {
    "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.in_reply_to_uri = ?1
         ORDER BY rs.published_at ASC"
}

async fn query_remote_statuses_with_actor(
    db: &D1Database,
    sql: &str,
    bindings: &[D1Type<'_>],
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let result = db.prepare(sql).bind_refs(bindings)?.all().await?;
    let values = result.results::<serde_json::Value>()?;
    Ok(values
        .into_iter()
        .filter_map(|value| {
            remote_status_row_from_value(&value)
                .ok()
                .map(|row| (row, RemoteActorRow::from_value(&value)))
        })
        .collect())
}

async fn query_remote_status_rows(
    db: &D1Database,
    sql: &str,
    bindings: &[D1Type<'_>],
) -> Result<Vec<RemoteStatusRow>> {
    let result = db.prepare(sql).bind_refs(bindings)?.all().await?;
    result
        .results::<RemoteStatusRecord>()
        .and_then(remote_statuses_from_records)
}

fn remote_status_row_from_value(value: &serde_json::Value) -> Result<RemoteStatusRow> {
    remote_status_from_record(RemoteStatusRecord {
        id: json_string(value, "id"),
        actor_uri: json_string(value, "actor_uri"),
        object_uri: json_string(value, "object_uri"),
        url: optional_json_string(value, "url"),
        in_reply_to_uri: optional_json_string(value, "in_reply_to_uri"),
        boost_of_uri: optional_json_string(value, "boost_of_uri"),
        quote_of_uri: optional_json_string(value, "quote_of_uri"),
        content_html: json_string(value, "content_html"),
        spoiler_text: json_string(value, "spoiler_text"),
        visibility: json_string_or(value, "visibility", "public"),
        sensitive: json_i32(value, "sensitive"),
        language: optional_json_string(value, "language"),
        quote_state: json_string_or(value, "quote_state", "accepted"),
        published_at: json_string(value, "published_at"),
    })
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    json_string_or(value, key, "")
}

fn json_string_or(value: &serde_json::Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

fn optional_json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn json_i32(value: &serde_json::Value, key: &str) -> i32 {
    value
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_unique_remote_status_tags_trims_normalizes_and_deduplicates() {
        let tags = normalized_unique_remote_status_tags(&[
            " Rust ".to_owned(),
            "#rust".to_owned(),
            "Masto".to_owned(),
            "  ".to_owned(),
        ]);

        assert_eq!(tags, ["rust", "masto"]);
    }

    #[test]
    fn remote_public_statuses_by_tags_indexed_sql_uses_tag_and_cursor_slots() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
            min_id: Some("status-min".to_owned()),
        };
        let tags = vec!["rust".to_owned(), "wasm".to_owned()];

        let (sql, bindings) = remote_public_statuses_by_tags_indexed_sql(&tags, &cursor, 20);

        assert!(sql.contains("h.tag IN (?1, ?2)"));
        assert!(sql.contains("rs.published_at <= ?3"));
        assert!(sql.contains("rs.id < ?4"));
        assert!(sql.contains("rs.published_at >= ?5"));
        assert!(sql.contains("rs.id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
        assert!(!sql.contains("IS NULL"));
        assert!(matches!(bindings[6], D1Type::Integer(20)));
    }

    #[test]
    fn remote_public_statuses_by_tags_legacy_patterns_preserve_fallback_shape() {
        let patterns = remote_public_statuses_by_tags_legacy_patterns(&[
            " Rust ".to_owned(),
            "#Masto".to_owned(),
            "  ".to_owned(),
        ]);

        assert_eq!(patterns, ["%#rust%", "%#masto%", "%#%"]);
    }

    #[test]
    fn remote_public_statuses_by_tags_legacy_sql_uses_pattern_and_cursor_slots() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
            min_id: Some("status-min".to_owned()),
        };
        let patterns = vec!["%#rust%".to_owned(), "%#masto%".to_owned()];

        let (sql, bindings) = remote_public_statuses_by_tags_legacy_sql(&patterns, &cursor, 13);

        assert!(sql.contains("lower(rs.content_html) LIKE ?1 OR lower(rs.content_html) LIKE ?2"));
        assert!(sql.contains("rs.published_at <= ?3"));
        assert!(sql.contains("rs.id < ?4"));
        assert!(sql.contains("LIMIT ?7"));
        assert!(!sql.contains("IS NULL"));
        assert!(matches!(bindings[6], D1Type::Integer(13)));
    }

    #[test]
    fn remote_public_statuses_by_link_patterns_wrap_urls_for_like_search() {
        let patterns = remote_public_statuses_by_link_patterns(&[
            "https://example.test/a".to_owned(),
            "acct:alice@example.test".to_owned(),
        ]);

        assert_eq!(
            patterns,
            ["%https://example.test/a%", "%acct:alice@example.test%"]
        );
    }

    #[test]
    fn remote_public_statuses_by_link_sql_uses_pattern_and_cursor_slots() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
            min_id: Some("status-min".to_owned()),
        };
        let patterns = vec![
            "%https://example.test/a%".to_owned(),
            "%acct:alice%".to_owned(),
        ];

        let (sql, bindings) = remote_public_statuses_by_link_sql(&patterns, &cursor, 15);

        assert!(sql.contains(
            "(rs.content_html LIKE ?1 OR rs.url LIKE ?1 OR rs.object_uri LIKE ?1) OR \
             (rs.content_html LIKE ?2 OR rs.url LIKE ?2 OR rs.object_uri LIKE ?2)"
        ));
        assert!(sql.contains("rs.published_at <= ?3"));
        assert!(sql.contains("LIMIT ?7"));
        assert!(!sql.contains("IS NULL"));
        assert!(matches!(bindings[6], D1Type::Integer(15)));
    }

    #[test]
    fn remote_direct_statuses_mentioning_viewer_sql_uses_seekable_cursor_slots() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: None,
            min_id: None,
        };
        let (sql, bindings) = remote_direct_statuses_mentioning_viewer_sql("@viewer", &cursor, 10);

        assert!(sql.contains("lower(rs.content_html) LIKE ?1"));
        assert!(sql.contains("rs.published_at <= ?2"));
        assert!(sql.contains("LIMIT ?4"));
        assert!(!sql.contains("IS NULL"));
        assert_eq!(bindings.len(), 4);
    }

    #[test]
    fn remote_actor_status_tagged_pattern_normalizes_and_discards_blank() {
        assert_eq!(
            remote_actor_status_tagged_pattern(Some(" RustLang ")).as_deref(),
            Some("%rustlang%")
        );
        assert!(remote_actor_status_tagged_pattern(Some("  ")).is_none());
        assert!(remote_actor_status_tagged_pattern(None).is_none());
    }

    #[test]
    fn remote_actor_statuses_by_actor_uri_bindings_keep_slots_stable() {
        let query_bindings = remote_actor_statuses_by_actor_uri_bindings(
            "actor",
            RemoteAccountStatusListOptions {
                max_id: Some("max"),
                min_id: Some("min"),
                limit: 20,
                visibility: AccountStatusVisibilityScope::All,
                only_media: false,
                exclude_replies: false,
                exclude_reblogs: false,
                tagged: Some("rust"),
            },
            Some("%rust%"),
        );

        assert!(matches!(query_bindings.bindings[0], D1Type::Text("actor")));
        assert!(matches!(query_bindings.bindings[1], D1Type::Text("max")));
        assert!(matches!(query_bindings.bindings[2], D1Type::Text("min")));
        assert!(matches!(query_bindings.bindings[3], D1Type::Text("%rust%")));
        assert!(matches!(query_bindings.bindings[4], D1Type::Integer(20)));
        assert_eq!(query_bindings.tagged_binding, Some(4));
        assert_eq!(query_bindings.limit_binding, 5);
    }

    #[test]
    fn remote_actor_statuses_by_actor_uri_bindings_omit_open_cursors() {
        let query_bindings = remote_actor_statuses_by_actor_uri_bindings(
            "actor",
            RemoteAccountStatusListOptions {
                max_id: None,
                min_id: None,
                limit: 8,
                visibility: AccountStatusVisibilityScope::All,
                only_media: false,
                exclude_replies: false,
                exclude_reblogs: false,
                tagged: None,
            },
            None,
        );

        assert_eq!(query_bindings.bindings.len(), 2);
        assert!(matches!(query_bindings.bindings[0], D1Type::Text("actor")));
        assert!(matches!(query_bindings.bindings[1], D1Type::Integer(8)));
        assert_eq!(query_bindings.tagged_binding, None);
        assert_eq!(query_bindings.limit_binding, 2);
    }

    #[test]
    fn remote_actor_status_filter_predicates_reflect_filters_and_tag_slot() {
        let predicates = remote_actor_status_filter_predicates(
            RemoteAccountStatusListOptions {
                max_id: Some("max"),
                min_id: Some("min"),
                limit: 20,
                visibility: AccountStatusVisibilityScope::PublicUnlistedPrivate,
                only_media: true,
                exclude_replies: true,
                exclude_reblogs: true,
                tagged: Some("rust"),
            },
            Some(4),
        );

        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "actor_uri = ?1")
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "visibility IN ('public', 'unlisted', 'private')")
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "boost_of_uri IS NULL")
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "in_reply_to_uri IS NULL")
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate.contains("remote_status_attachments media"))
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "lower(content_html) LIKE ?4")
        );
    }

    #[test]
    fn remote_actor_statuses_by_actor_uri_sql_uses_predicates_and_limit_slot() {
        let predicates = vec![
            "actor_uri = ?1".to_owned(),
            "boost_of_uri IS NULL".to_owned(),
        ];
        let cursor_parts = crate::StatusIdCursorParts {
            with_clauses: vec!["max_cursor AS (SELECT id, created_at FROM remote_statuses WHERE id = ?2 LIMIT 1)".to_owned()],
            predicates: vec!["EXISTS (SELECT 1 FROM max_cursor WHERE remote_statuses.published_at < max_cursor.published_at)".to_owned()],
        };
        let sql = remote_actor_statuses_by_actor_uri_sql(&predicates, &cursor_parts, 3);

        assert!(sql.contains("WITH max_cursor AS"));
        assert!(sql.contains("WHERE actor_uri = ?1\n           AND boost_of_uri IS NULL"));
        assert!(sql.contains("LIMIT ?3"));
        assert!(!sql.contains("?2 IS NULL"));
        assert!(sql.contains("boost_of_uri IS NULL"));
    }

    #[test]
    fn public_remote_statuses_by_actor_uri_sql_uses_seekable_id_cursors() {
        let (sql, bindings) =
            public_remote_statuses_by_actor_uri_sql("actor", Some("max"), Some("min"), 20);

        assert!(sql.contains("WHERE actor_uri = ?1"));
        assert!(sql.contains("max_cursor AS"));
        assert!(sql.contains("min_cursor AS"));
        assert!(sql.contains("visibility IN ('public', 'unlisted')"));
        assert!(sql.contains("LIMIT ?4"));
        assert!(!sql.contains("IS NULL"));
        assert!(matches!(bindings[0], D1Type::Text("actor")));
        assert!(matches!(bindings[1], D1Type::Text("max")));
        assert!(matches!(bindings[2], D1Type::Text("min")));
        assert!(matches!(bindings[3], D1Type::Integer(20)));
    }

    #[test]
    fn direct_remote_replies_by_uri_sql_uses_reply_slot_and_ascending_order() {
        let sql = direct_remote_replies_by_uri_sql();

        assert!(sql.contains("JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri"));
        assert!(sql.contains("WHERE rs.in_reply_to_uri = ?1"));
        assert!(sql.contains("ORDER BY rs.published_at ASC"));
    }

    #[test]
    fn remote_public_timeline_statuses_sql_uses_seekable_cursor_slots_and_limit() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: None,
            min_id: None,
        };
        let (sql, bindings) = remote_public_timeline_statuses_sql(&cursor, 20);

        assert!(sql.contains("WHERE rs.visibility = 'public'"));
        assert!(sql.contains("rs.published_at <= ?1"));
        assert!(sql.contains("rs.id < ?2"));
        assert!(sql.contains("LIMIT ?3"));
        assert!(!sql.contains("IS NULL"));
        assert_eq!(bindings.len(), 3);
    }

    #[test]
    fn remote_home_timeline_statuses_sql_uses_viewer_and_seekable_cursor_slots() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
            min_id: Some("status-min".to_owned()),
        };
        let (sql, bindings) = remote_home_timeline_statuses_sql("viewer", &cursor, 20);

        assert!(sql.contains("f.follower_account_id = ?1"));
        assert!(sql.contains("f.state = 'accepted'"));
        assert!(sql.contains("WHERE rs.visibility IN ('public', 'unlisted', 'private')"));
        assert!(sql.contains("rs.published_at <= ?2"));
        assert!(sql.contains("rs.published_at >= ?4"));
        assert!(sql.contains("LIMIT ?6"));
        assert!(!sql.contains("IS NULL"));
        assert_eq!(bindings.len(), 6);
    }

    #[test]
    fn remote_statuses_with_actors_by_ids_sql_uses_json_each() {
        let sql = remote_statuses_with_actors_by_ids_sql();

        assert!(sql.contains("JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri"));
        assert!(sql.contains("WHERE rs.id IN (SELECT value FROM json_each(?1))"));
    }

    #[test]
    fn remote_status_row_from_value_maps_optional_and_default_fields() {
        let row = remote_status_row_from_value(&serde_json::json!({
            "id": "status-1",
            "actor_uri": "https://remote.example/users/alice",
            "object_uri": "https://remote.example/users/alice/statuses/1",
            "url": "https://remote.example/@alice/1",
            "in_reply_to_uri": "https://remote.example/statuses/root",
            "boost_of_uri": "https://remote.example/statuses/boost",
            "quote_of_uri": "https://remote.example/statuses/quote",
            "content_html": "<p>Hello</p>",
            "spoiler_text": "CW",
            "visibility": "unlisted",
            "sensitive": 1,
            "language": "ja",
            "quote_state": "pending",
            "published_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap();

        assert_eq!(row.id, "status-1");
        assert_eq!(row.actor_uri, "https://remote.example/users/alice");
        assert_eq!(
            row.object_uri,
            "https://remote.example/users/alice/statuses/1"
        );
        assert_eq!(row.url.as_deref(), Some("https://remote.example/@alice/1"));
        assert_eq!(
            row.in_reply_to_uri.as_deref(),
            Some("https://remote.example/statuses/root")
        );
        assert_eq!(
            row.boost_of_uri.as_deref(),
            Some("https://remote.example/statuses/boost")
        );
        assert_eq!(
            row.quote_of_uri.as_deref(),
            Some("https://remote.example/statuses/quote")
        );
        assert_eq!(row.content_html, "<p>Hello</p>");
        assert_eq!(row.spoiler_text, "CW");
        assert_eq!(row.visibility, cfwdon_domain::Visibility::Unlisted);
        assert!(row.sensitive);
        assert_eq!(row.language.as_deref(), Some("ja"));
        assert_eq!(row.quote_state, cfwdon_domain::QuoteState::Pending);
        assert_eq!(row.published_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn remote_status_row_from_value_uses_empty_and_quote_state_defaults() {
        let row = remote_status_row_from_value(&serde_json::json!({})).unwrap();

        assert_eq!(row.id, "");
        assert_eq!(row.actor_uri, "");
        assert_eq!(row.object_uri, "");
        assert!(row.url.is_none());
        assert!(row.in_reply_to_uri.is_none());
        assert!(row.boost_of_uri.is_none());
        assert!(row.quote_of_uri.is_none());
        assert_eq!(row.content_html, "");
        assert_eq!(row.spoiler_text, "");
        assert_eq!(row.visibility, cfwdon_domain::Visibility::Public);
        assert!(!row.sensitive);
        assert!(row.language.is_none());
        assert_eq!(row.quote_state, cfwdon_domain::QuoteState::Accepted);
        assert_eq!(row.published_at, "");
    }
}
