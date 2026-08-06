use super::{
    D1Database, ResolvedTimelineCursor, Result, StatusRecord, StatusRow, normalize_hashtag,
    status_from_record, statuses_from_records,
};
use crate::{
    append_min_timestamp_cursor_bindings, append_resolved_timeline_cursor_bindings,
    seekable_min_timestamp_cursor_predicates, seekable_resolved_timeline_cursor_predicates,
};
use std::collections::HashSet;
use worker::d1::D1Type;

const LOCAL_STATUS_COLUMNS: &str = "id, account_id, ap_id, in_reply_to_id, in_reply_to_account_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, application_id, card_json, created_at, updated_at";
const LOCAL_STATUS_S_SELECT: &str = "s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.in_reply_to_account_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.application_id, s.card_json, s.created_at, s.updated_at";

pub(crate) async fn list_local_home_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    if cursor.max_timestamp.is_none()
        && let Some(min_timestamp) = cursor.min_timestamp.as_deref()
    {
        return list_local_home_timeline_statuses_since(
            db,
            viewer_account_id,
            min_timestamp,
            cursor.min_id.as_deref(),
            limit,
        )
        .await;
    }

    let (sql, bindings) = local_home_timeline_sql(viewer_account_id, cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    crate::d1_results::<StatusRecord>(&result).and_then(statuses_from_records)
}

async fn list_local_home_timeline_statuses_since(
    db: &D1Database,
    viewer_account_id: &str,
    min_timestamp: &str,
    min_id: Option<&str>,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let (sql, bindings) =
        local_home_timeline_since_sql(viewer_account_id, min_timestamp, min_id, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    crate::d1_results::<StatusRecord>(&result).and_then(statuses_from_records)
}

fn local_home_timeline_sql<'a>(
    viewer_account_id: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = vec![
        D1Type::Text(viewer_account_id),
        D1Type::Text(viewer_account_id),
    ];
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("s.created_at", "s.id", &slots);
    let sql = format!(
        "SELECT {LOCAL_STATUS_COLUMNS}
             FROM (
                SELECT {LOCAL_STATUS_S_SELECT}
                FROM statuses s
                WHERE s.account_id = ?1{cursor_predicates}

                UNION

                SELECT {LOCAL_STATUS_S_SELECT}
                FROM follows f
                JOIN statuses s
                  ON s.account_id = f.target_account_id
                WHERE f.follower_account_id = ?2
                  AND f.state = 'accepted'
                  AND s.visibility IN ('public', 'unlisted', 'private'){cursor_predicates}
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

fn local_home_timeline_since_sql<'a>(
    viewer_account_id: &'a str,
    min_timestamp: &'a str,
    min_id: Option<&'a str>,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = vec![
        D1Type::Text(viewer_account_id),
        D1Type::Text(viewer_account_id),
    ];
    let slots = append_min_timestamp_cursor_bindings(&mut bindings, min_timestamp, min_id);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_min_timestamp_cursor_predicates("s.created_at", "s.id", &slots);
    let sql = format!(
        "SELECT {LOCAL_STATUS_COLUMNS}
             FROM (
                SELECT {LOCAL_STATUS_S_SELECT}
                FROM statuses s
                WHERE s.account_id = ?1{cursor_predicates}

                UNION

                SELECT {LOCAL_STATUS_S_SELECT}
                FROM follows f
                JOIN statuses s
                  ON s.account_id = f.target_account_id
                WHERE f.follower_account_id = ?2
                  AND f.state = 'accepted'
                  AND s.visibility IN ('public', 'unlisted', 'private'){cursor_predicates}
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

pub(crate) async fn list_local_public_timeline_statuses(
    db: &D1Database,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let (sql, bindings) = local_public_timeline_sql(cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    crate::d1_results::<StatusRecord>(&result).and_then(statuses_from_records)
}

fn local_public_timeline_sql<'a>(
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = Vec::new();
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("created_at", "id", &slots);
    let sql = format!(
        "SELECT {LOCAL_STATUS_COLUMNS}
         FROM statuses
         WHERE visibility = 'public'{cursor_predicates}
         ORDER BY created_at DESC, id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

pub(crate) async fn list_local_public_statuses_by_tag(
    db: &D1Database,
    tag: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    list_local_public_statuses_by_tags(db, &[normalize_hashtag(tag)], cursor, limit).await
}

pub(crate) async fn list_local_public_statuses_by_tags(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let (mut rows, tags) =
        list_local_public_statuses_by_tags_indexed(db, tags, cursor, limit).await?;
    if rows.len() >= limit as usize {
        return Ok(rows);
    }
    let mut seen_ids = rows
        .iter()
        .map(|status| status.id.clone())
        .collect::<HashSet<_>>();
    for status in list_local_public_statuses_by_tags_legacy(db, &tags, cursor, limit).await? {
        if seen_ids.insert(status.id.clone()) {
            rows.push(status);
        }
    }
    rows.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    rows.truncate(limit as usize);
    Ok(rows)
}

async fn list_local_public_statuses_by_tags_indexed(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<(Vec<StatusRow>, Vec<String>)> {
    let tags = normalize_unique_tags(tags);
    if tags.is_empty() {
        return Ok((Vec::new(), tags));
    }

    let (sql, bindings) = local_public_statuses_by_tags_indexed_sql(&tags, cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok((
        crate::d1_results::<StatusRecord>(&result)?
            .into_iter()
            .map(status_from_record)
            .collect::<Result<Vec<_>>>()?,
        tags,
    ))
}

fn normalize_unique_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.iter()
        .map(|tag| normalize_hashtag(tag))
        .filter(|tag| !tag.is_empty() && seen.insert(tag.clone()))
        .collect()
}

fn local_public_statuses_by_tags_indexed_sql<'a>(
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
        seekable_resolved_timeline_cursor_predicates("created_at", "id", &slots);
    let sql = format!(
        "SELECT {LOCAL_STATUS_COLUMNS}
         FROM statuses
         WHERE visibility = 'public'
           AND id IN (
               SELECT h.status_id
               FROM status_hashtags h
               WHERE h.tag IN ({tag_placeholders})
           ){cursor_predicates}
         ORDER BY created_at DESC, id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

async fn list_local_public_statuses_by_tags_legacy(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let patterns = local_public_statuses_by_tags_legacy_patterns(tags);
    let (sql, bindings) = local_public_statuses_by_tags_legacy_sql(&patterns, cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    crate::d1_results::<StatusRecord>(&result).and_then(statuses_from_records)
}

fn local_public_statuses_by_tags_legacy_patterns(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| format!("%#{}%", normalize_hashtag(tag)))
        .collect()
}

fn local_public_statuses_by_tags_legacy_sql<'a>(
    patterns: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let match_clause = (1..=patterns.len())
        .map(|index| format!("lower(text_content) LIKE ?{index}"))
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
        seekable_resolved_timeline_cursor_predicates("created_at", "id", &slots);
    let sql = format!(
        "SELECT {LOCAL_STATUS_COLUMNS}
         FROM statuses
         WHERE visibility = 'public'
           AND ({match_clause}){cursor_predicates}
         ORDER BY created_at DESC, id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

pub(crate) async fn list_local_public_statuses_by_link(
    db: &D1Database,
    urls: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    if urls.is_empty() {
        return Ok(Vec::new());
    }

    let patterns = local_public_statuses_by_link_patterns(urls);
    let (sql, bindings) = local_public_statuses_by_link_sql(&patterns, cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    crate::d1_results::<StatusRecord>(&result).and_then(statuses_from_records)
}

fn local_public_statuses_by_link_patterns(urls: &[String]) -> Vec<String> {
    urls.iter().map(|url| format!("%{url}%")).collect()
}

fn local_public_statuses_by_link_sql<'a>(
    patterns: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let match_clause = (1..=patterns.len())
        .map(|position| {
            format!("(s.text_content LIKE ?{position} OR s.content_html LIKE ?{position})")
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
        seekable_resolved_timeline_cursor_predicates("s.created_at", "s.id", &slots);
    let sql = format!(
        "SELECT {LOCAL_STATUS_S_SELECT}
         FROM statuses s
         JOIN accounts a ON a.id = s.account_id
         WHERE s.visibility = 'public'
           AND a.discoverable = 1
           AND ({match_clause}){cursor_predicates}
         ORDER BY s.created_at DESC, s.id DESC
         LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

pub(crate) async fn list_local_direct_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let (sql, bindings) = local_direct_timeline_sql(viewer_account_id, cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    crate::d1_results::<StatusRecord>(&result).and_then(statuses_from_records)
}

fn local_direct_timeline_sql<'a>(
    viewer_account_id: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = vec![D1Type::Text(viewer_account_id)];
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("s.created_at", "s.id", &slots);
    let sql = format!(
        "SELECT DISTINCT {LOCAL_STATUS_S_SELECT}
             FROM statuses s
             JOIN conversation_statuses cs
               ON cs.status_id = s.id
             JOIN conversation_states cst
               ON cst.conversation_id = cs.conversation_id
              AND cst.account_id = ?1
              AND cst.deleted_at IS NULL
             WHERE s.visibility = 'direct'{cursor_predicates}
             ORDER BY s.created_at DESC, s.id DESC
             LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cursor() -> ResolvedTimelineCursor {
        ResolvedTimelineCursor {
            max_timestamp: None,
            max_id: None,
            min_timestamp: None,
            min_id: None,
        }
    }

    #[test]
    fn local_home_timeline_sql_keeps_slot_order_stable_with_full_cursor() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());

        let (sql, bindings) = local_home_timeline_sql("viewer", &cursor, 12);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Text("viewer")));
        assert!(matches!(bindings[2], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[3], D1Type::Text("status-max")));
        assert!(matches!(bindings[4], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[5], D1Type::Text("status-min")));
        assert!(matches!(bindings[6], D1Type::Integer(12)));
        assert!(sql.contains("s.created_at <= ?3"));
        assert!(sql.contains("LIMIT ?7"));
        assert!(!sql.contains("IS NULL"));
    }

    #[test]
    fn local_home_timeline_sql_omits_cursor_predicates_for_open_bounds() {
        let cursor = empty_cursor();
        let (sql, bindings) = local_home_timeline_sql("viewer", &cursor, 8);

        assert_eq!(bindings.len(), 3);
        assert!(!sql.contains("IS NULL"));
        assert!(!sql.contains("<= ?"));
        assert!(sql.contains("LIMIT ?3"));
    }

    #[test]
    fn local_home_timeline_since_sql_keeps_slot_order_stable() {
        let (sql, bindings) =
            local_home_timeline_since_sql("viewer", "2026-01-01T00:00:00Z", Some("status-min"), 10);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Text("viewer")));
        assert!(matches!(bindings[2], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[3], D1Type::Text("status-min")));
        assert!(matches!(bindings[4], D1Type::Integer(10)));
        assert!(sql.contains("s.created_at > ?3"));
        assert!(sql.contains("LIMIT ?5"));
    }

    #[test]
    fn local_home_timeline_since_sql_omits_id_tie_break_without_min_id() {
        let (sql, bindings) =
            local_home_timeline_since_sql("viewer", "2026-01-01T00:00:00Z", None, 10);

        assert_eq!(bindings.len(), 4);
        assert!(sql.contains("s.created_at > ?3"));
        assert!(!sql.contains("s.id >"));
    }

    #[test]
    fn local_public_timeline_sql_keeps_slot_order_stable_with_full_cursor() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());

        let (sql, bindings) = local_public_timeline_sql(&cursor, 14);

        assert!(matches!(bindings[0], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[1], D1Type::Text("status-max")));
        assert!(matches!(bindings[2], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[3], D1Type::Text("status-min")));
        assert!(matches!(bindings[4], D1Type::Integer(14)));
        assert!(sql.contains("created_at <= ?1"));
        assert!(sql.contains("LIMIT ?5"));
        assert!(!sql.contains("IS NULL"));
    }

    #[test]
    fn local_public_timeline_sql_omits_cursor_predicates_for_open_bounds() {
        let cursor = empty_cursor();
        let (sql, bindings) = local_public_timeline_sql(&cursor, 6);

        assert_eq!(bindings.len(), 1);
        assert!(!sql.contains("IS NULL"));
        assert!(sql.contains("LIMIT ?1"));
    }

    #[test]
    fn normalize_unique_tags_keeps_first_normalized_tag() {
        let tags = vec![
            "#Rust".to_owned(),
            "rust".to_owned(),
            " wasm ".to_owned(),
            "#".to_owned(),
            "WASM".to_owned(),
        ];

        let normalized = normalize_unique_tags(&tags);

        assert_eq!(normalized, vec!["rust".to_owned(), "wasm".to_owned()]);
    }

    #[test]
    fn local_public_statuses_by_tags_indexed_sql_offsets_cursor_slots_after_tags() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());
        let tags = vec!["rust".to_owned(), "wasm".to_owned()];

        let (sql, bindings) = local_public_statuses_by_tags_indexed_sql(&tags, &cursor, 20);

        assert!(sql.contains("WHERE h.tag IN (?1, ?2)"));
        assert!(sql.contains("created_at <= ?3"));
        assert!(sql.contains("id < ?4"));
        assert!(sql.contains("created_at >= ?5"));
        assert!(sql.contains("id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
        assert!(!sql.contains("IS NULL"));
        assert!(matches!(bindings[0], D1Type::Text("rust")));
        assert!(matches!(bindings[1], D1Type::Text("wasm")));
        assert!(matches!(bindings[6], D1Type::Integer(20)));
    }

    #[test]
    fn local_public_statuses_by_tags_indexed_sql_omits_open_cursor_predicates() {
        let cursor = empty_cursor();
        let tags = vec!["rust".to_owned()];

        let (sql, bindings) = local_public_statuses_by_tags_indexed_sql(&tags, &cursor, 9);

        assert_eq!(bindings.len(), 2);
        assert!(!sql.contains("IS NULL"));
        assert!(sql.contains("LIMIT ?2"));
    }

    #[test]
    fn local_public_statuses_by_tags_legacy_patterns_preserve_fallback_shape() {
        let patterns = local_public_statuses_by_tags_legacy_patterns(&[
            " Rust ".to_owned(),
            "#Masto".to_owned(),
            "  ".to_owned(),
        ]);

        assert_eq!(patterns, ["%#rust%", "%#masto%", "%#%"]);
    }

    #[test]
    fn local_public_statuses_by_tags_legacy_sql_uses_pattern_and_cursor_slots() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());
        let patterns = vec!["%#rust%".to_owned(), "%#masto%".to_owned()];

        let (sql, bindings) = local_public_statuses_by_tags_legacy_sql(&patterns, &cursor, 13);

        assert!(sql.contains("lower(text_content) LIKE ?1 OR lower(text_content) LIKE ?2"));
        assert!(sql.contains("created_at <= ?3"));
        assert!(sql.contains("id < ?4"));
        assert!(sql.contains("created_at >= ?5"));
        assert!(sql.contains("id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
        assert!(!sql.contains("IS NULL"));
        assert!(matches!(bindings[0], D1Type::Text("%#rust%")));
        assert!(matches!(bindings[1], D1Type::Text("%#masto%")));
        assert!(matches!(bindings[6], D1Type::Integer(13)));
    }

    #[test]
    fn local_public_statuses_by_link_patterns_wrap_urls_for_like_search() {
        let patterns = local_public_statuses_by_link_patterns(&[
            "https://example.test/a".to_owned(),
            "acct:alice@example.test".to_owned(),
        ]);

        assert_eq!(
            patterns,
            ["%https://example.test/a%", "%acct:alice@example.test%"]
        );
    }

    #[test]
    fn local_public_statuses_by_link_sql_uses_pattern_and_cursor_slots() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());
        let patterns = vec![
            "%https://example.test/a%".to_owned(),
            "%acct:alice@example.test%".to_owned(),
        ];

        let (sql, bindings) = local_public_statuses_by_link_sql(&patterns, &cursor, 15);

        assert!(sql.contains(
            "(s.text_content LIKE ?1 OR s.content_html LIKE ?1) OR \
             (s.text_content LIKE ?2 OR s.content_html LIKE ?2)"
        ));
        assert!(sql.contains("s.created_at <= ?3"));
        assert!(sql.contains("s.id < ?4"));
        assert!(sql.contains("s.created_at >= ?5"));
        assert!(sql.contains("s.id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
        assert!(!sql.contains("IS NULL"));
        assert!(matches!(bindings[6], D1Type::Integer(15)));
    }

    #[test]
    fn local_direct_timeline_sql_keeps_slot_order_stable_with_full_cursor() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());

        let (sql, bindings) = local_direct_timeline_sql("viewer", &cursor, 11);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[2], D1Type::Text("status-max")));
        assert!(matches!(bindings[3], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[4], D1Type::Text("status-min")));
        assert!(matches!(bindings[5], D1Type::Integer(11)));
        assert!(sql.contains("s.created_at <= ?2"));
        assert!(sql.contains("LIMIT ?6"));
        assert!(!sql.contains("?2 IS NULL"));
    }

    #[test]
    fn local_direct_timeline_sql_omits_cursor_predicates_for_open_bounds() {
        let cursor = empty_cursor();
        let (sql, bindings) = local_direct_timeline_sql("viewer", &cursor, 5);

        assert_eq!(bindings.len(), 2);
        assert!(!sql.contains("?2 IS NULL"));
        assert!(sql.contains("deleted_at IS NULL"));
        assert!(sql.contains("LIMIT ?2"));
    }
}
