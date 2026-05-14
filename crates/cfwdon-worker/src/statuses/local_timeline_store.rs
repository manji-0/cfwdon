use super::{D1Database, ResolvedTimelineCursor, Result, StatusRow, normalize_hashtag};
use std::collections::HashSet;
use worker::d1::D1Type;

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

    let bindings = local_home_timeline_bindings(viewer_account_id, cursor, limit);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at, updated_at
             FROM (
                SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at, s.updated_at
                FROM statuses s
                WHERE s.account_id = ?1
                  AND (
                       ?3 IS NULL
                       OR s.created_at < ?3
                       OR (s.created_at = ?3 AND s.id < ?4)
                  )
                  AND (
                       ?5 IS NULL
                       OR s.created_at > ?5
                       OR (s.created_at = ?5 AND s.id > ?6)
                  )

                UNION

                SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at, s.updated_at
                FROM follows f
                JOIN statuses s
                  ON s.account_id = f.target_account_id
                WHERE f.follower_account_id = ?2
                  AND f.state = 'accepted'
                  AND s.visibility IN ('public', 'unlisted', 'private')
                  AND (
                       ?3 IS NULL
                       OR s.created_at < ?3
                       OR (s.created_at = ?3 AND s.id < ?4)
                  )
                  AND (
                       ?5 IS NULL
                       OR s.created_at > ?5
                       OR (s.created_at = ?5 AND s.id > ?6)
                  )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?7",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusRow>()
}

async fn list_local_home_timeline_statuses_since(
    db: &D1Database,
    viewer_account_id: &str,
    min_timestamp: &str,
    min_id: Option<&str>,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings =
        local_home_timeline_since_bindings(viewer_account_id, min_timestamp, min_id, limit);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at, updated_at
             FROM (
                SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at, s.updated_at
                FROM statuses s
                WHERE s.account_id = ?1
                  AND (
                       s.created_at > ?3
                       OR (s.created_at = ?3 AND (?4 IS NULL OR s.id > ?4))
                  )

                UNION

                SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at, s.updated_at
                FROM follows f
                JOIN statuses s
                  ON s.account_id = f.target_account_id
                WHERE f.follower_account_id = ?2
                  AND f.state = 'accepted'
                  AND s.visibility IN ('public', 'unlisted', 'private')
                  AND (
                       s.created_at > ?3
                       OR (s.created_at = ?3 AND (?4 IS NULL OR s.id > ?4))
                  )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?5",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusRow>()
}

fn local_home_timeline_bindings<'a>(
    viewer_account_id: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> [D1Type<'a>; 7] {
    [
        D1Type::Text(viewer_account_id),
        D1Type::Text(viewer_account_id),
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ]
}

fn local_home_timeline_since_bindings<'a>(
    viewer_account_id: &'a str,
    min_timestamp: &'a str,
    min_id: Option<&'a str>,
    limit: u32,
) -> [D1Type<'a>; 5] {
    [
        D1Type::Text(viewer_account_id),
        D1Type::Text(viewer_account_id),
        D1Type::Text(min_timestamp),
        min_id.map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ]
}

pub(crate) async fn list_local_public_timeline_statuses(
    db: &D1Database,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = local_public_timeline_bindings(cursor, limit);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at, updated_at
             FROM statuses
             WHERE visibility = 'public'
               AND (
                    ?1 IS NULL
                    OR created_at < ?1
                    OR (created_at = ?1 AND id < ?2)
               )
               AND (
                    ?3 IS NULL
                    OR created_at > ?3
                    OR (created_at = ?3 AND id > ?4)
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?5",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusRow>()
}

fn local_public_timeline_bindings<'a>(
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> [D1Type<'a>; 5] {
    [
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ]
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

    let sql = local_public_statuses_by_tags_indexed_sql(tags.len());
    let bindings = local_public_statuses_by_tags_indexed_bindings(&tags, cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok((result.results::<StatusRow>()?, tags))
}

fn normalize_unique_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.iter()
        .map(|tag| normalize_hashtag(tag))
        .filter(|tag| !tag.is_empty() && seen.insert(tag.clone()))
        .collect()
}

fn local_public_statuses_by_tags_indexed_sql(tag_count: usize) -> String {
    let tag_placeholders = (1..=tag_count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let max_timestamp_index = tag_count + 1;
    let max_id_index = tag_count + 2;
    let min_timestamp_index = tag_count + 3;
    let min_id_index = tag_count + 4;
    let limit_index = tag_count + 5;
    format!(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at, updated_at
         FROM statuses
         WHERE visibility = 'public'
           AND id IN (
               SELECT h.status_id
               FROM status_hashtags h
               WHERE h.tag IN ({tag_placeholders})
           )
           AND (
                ?{max_timestamp_index} IS NULL
                OR created_at < ?{max_timestamp_index}
                OR (created_at = ?{max_timestamp_index} AND id < ?{max_id_index})
           )
           AND (
                ?{min_timestamp_index} IS NULL
                OR created_at > ?{min_timestamp_index}
                OR (created_at = ?{min_timestamp_index} AND id > ?{min_id_index})
           )
         ORDER BY created_at DESC, id DESC
         LIMIT ?{limit_index}"
    )
}

fn local_public_statuses_by_tags_indexed_bindings<'a>(
    tags: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> Vec<D1Type<'a>> {
    let mut bindings = tags
        .iter()
        .map(|tag| D1Type::Text(tag.as_str()))
        .collect::<Vec<_>>();
    bindings.extend([
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ]);
    bindings
}

async fn list_local_public_statuses_by_tags_legacy(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let patterns = local_public_statuses_by_tags_legacy_patterns(tags);
    let sql = local_public_statuses_by_tags_legacy_sql(patterns.len());
    let bindings = local_public_statuses_by_tags_legacy_bindings(&patterns, cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    result.results::<StatusRow>()
}

fn local_public_statuses_by_tags_legacy_patterns(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| format!("%#{}%", normalize_hashtag(tag)))
        .collect()
}

fn local_public_statuses_by_tags_legacy_sql(pattern_count: usize) -> String {
    let match_clause = (1..=pattern_count)
        .map(|index| format!("lower(text_content) LIKE ?{index}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let max_timestamp_index = pattern_count + 1;
    let max_id_index = pattern_count + 2;
    let min_timestamp_index = pattern_count + 3;
    let min_id_index = pattern_count + 4;
    let limit_index = pattern_count + 5;
    format!(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at, updated_at
         FROM statuses
         WHERE visibility = 'public'
           AND ({match_clause})
           AND (
                ?{max_timestamp_index} IS NULL
                OR created_at < ?{max_timestamp_index}
                OR (created_at = ?{max_timestamp_index} AND id < ?{max_id_index})
           )
           AND (
                ?{min_timestamp_index} IS NULL
                OR created_at > ?{min_timestamp_index}
                OR (created_at = ?{min_timestamp_index} AND id > ?{min_id_index})
           )
         ORDER BY created_at DESC, id DESC
         LIMIT ?{limit_index}"
    )
}

fn local_public_statuses_by_tags_legacy_bindings<'a>(
    patterns: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> Vec<D1Type<'a>> {
    let mut bindings = patterns
        .iter()
        .map(|pattern| D1Type::Text(pattern.as_str()))
        .collect::<Vec<_>>();
    bindings.extend([
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ]);
    bindings
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
    let sql = local_public_statuses_by_link_sql(patterns.len());
    let bindings = local_public_statuses_by_link_bindings(&patterns, cursor, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    result.results::<StatusRow>()
}

fn local_public_statuses_by_link_patterns(urls: &[String]) -> Vec<String> {
    urls.iter().map(|url| format!("%{url}%")).collect()
}

fn local_public_statuses_by_link_sql(pattern_count: usize) -> String {
    let match_clause = (1..=pattern_count)
        .map(|position| {
            format!("(s.text_content LIKE ?{position} OR s.content_html LIKE ?{position})")
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let cursor_offset = pattern_count;
    format!(
        "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at, s.updated_at
         FROM statuses s
         JOIN accounts a ON a.id = s.account_id
         WHERE s.visibility = 'public'
           AND a.discoverable = 1
           AND ({match_clause})
           AND (
                ?{max_timestamp} IS NULL
                OR s.created_at < ?{max_timestamp}
                OR (s.created_at = ?{max_timestamp} AND s.id < ?{max_id})
           )
           AND (
                ?{min_timestamp} IS NULL
                OR s.created_at > ?{min_timestamp}
                OR (s.created_at = ?{min_timestamp} AND s.id > ?{min_id})
           )
         ORDER BY s.created_at DESC, s.id DESC
         LIMIT ?{limit_position}",
        max_timestamp = cursor_offset + 1,
        max_id = cursor_offset + 2,
        min_timestamp = cursor_offset + 3,
        min_id = cursor_offset + 4,
        limit_position = cursor_offset + 5,
    )
}

fn local_public_statuses_by_link_bindings<'a>(
    patterns: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> Vec<D1Type<'a>> {
    let mut bindings = patterns
        .iter()
        .map(|pattern| D1Type::Text(pattern.as_str()))
        .collect::<Vec<_>>();
    bindings.push(
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
    );
    bindings.push(cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text));
    bindings.push(
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
    );
    bindings.push(cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text));
    bindings.push(D1Type::Integer(limit as i32));
    bindings
}

pub(crate) async fn list_local_direct_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = local_direct_timeline_bindings(viewer_account_id, cursor, limit);
    let result = db
        .prepare(
            "SELECT DISTINCT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at, s.updated_at
             FROM statuses s
             JOIN conversation_statuses cs
               ON cs.status_id = s.id
             JOIN conversation_states cst
               ON cst.conversation_id = cs.conversation_id
              AND cst.account_id = ?1
              AND cst.deleted_at IS NULL
             WHERE s.visibility = 'direct'
               AND (
                    ?2 IS NULL
                    OR s.created_at < ?2
                    OR (s.created_at = ?2 AND s.id < ?3)
               )
               AND (
                    ?4 IS NULL
                    OR s.created_at > ?4
                    OR (s.created_at = ?4 AND s.id > ?5)
               )
             ORDER BY s.created_at DESC, s.id DESC
             LIMIT ?6",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusRow>()
}

fn local_direct_timeline_bindings<'a>(
    viewer_account_id: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> [D1Type<'a>; 6] {
    [
        D1Type::Text(viewer_account_id),
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ]
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
    fn local_home_timeline_bindings_keep_sql_slot_order_stable() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());

        let bindings = local_home_timeline_bindings("viewer", &cursor, 12);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Text("viewer")));
        assert!(matches!(bindings[2], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[3], D1Type::Text("status-max")));
        assert!(matches!(bindings[4], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[5], D1Type::Text("status-min")));
        assert!(matches!(bindings[6], D1Type::Integer(12)));
    }

    #[test]
    fn local_home_timeline_bindings_use_null_for_open_cursor_bounds() {
        let cursor = empty_cursor();
        let bindings = local_home_timeline_bindings("viewer", &cursor, 8);

        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(bindings[5], D1Type::Null));
        assert!(matches!(bindings[6], D1Type::Integer(8)));
    }

    #[test]
    fn local_home_timeline_since_bindings_keep_sql_slot_order_stable() {
        let bindings = local_home_timeline_since_bindings(
            "viewer",
            "2026-01-01T00:00:00Z",
            Some("status-min"),
            10,
        );

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Text("viewer")));
        assert!(matches!(bindings[2], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[3], D1Type::Text("status-min")));
        assert!(matches!(bindings[4], D1Type::Integer(10)));
    }

    #[test]
    fn local_home_timeline_since_bindings_use_null_for_open_min_id() {
        let bindings =
            local_home_timeline_since_bindings("viewer", "2026-01-01T00:00:00Z", None, 10);

        assert!(matches!(bindings[3], D1Type::Null));
    }

    #[test]
    fn local_public_timeline_bindings_keep_sql_slot_order_stable() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());

        let bindings = local_public_timeline_bindings(&cursor, 14);

        assert!(matches!(bindings[0], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[1], D1Type::Text("status-max")));
        assert!(matches!(bindings[2], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[3], D1Type::Text("status-min")));
        assert!(matches!(bindings[4], D1Type::Integer(14)));
    }

    #[test]
    fn local_public_timeline_bindings_use_null_for_open_cursor_bounds() {
        let cursor = empty_cursor();
        let bindings = local_public_timeline_bindings(&cursor, 6);

        assert!(matches!(bindings[0], D1Type::Null));
        assert!(matches!(bindings[1], D1Type::Null));
        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Integer(6)));
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
        let sql = local_public_statuses_by_tags_indexed_sql(2);

        assert!(sql.contains("WHERE h.tag IN (?1, ?2)"));
        assert!(sql.contains("?3 IS NULL"));
        assert!(sql.contains("id < ?4"));
        assert!(sql.contains("?5 IS NULL"));
        assert!(sql.contains("id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
    }

    #[test]
    fn local_public_statuses_by_tags_indexed_bindings_keep_sql_slot_order_stable() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());
        let tags = vec!["rust".to_owned(), "wasm".to_owned()];

        let bindings = local_public_statuses_by_tags_indexed_bindings(&tags, &cursor, 20);

        assert!(matches!(bindings[0], D1Type::Text("rust")));
        assert!(matches!(bindings[1], D1Type::Text("wasm")));
        assert!(matches!(bindings[2], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[3], D1Type::Text("status-max")));
        assert!(matches!(bindings[4], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[5], D1Type::Text("status-min")));
        assert!(matches!(bindings[6], D1Type::Integer(20)));
    }

    #[test]
    fn local_public_statuses_by_tags_indexed_bindings_use_null_for_open_cursor_bounds() {
        let cursor = empty_cursor();
        let tags = vec!["rust".to_owned()];

        let bindings = local_public_statuses_by_tags_indexed_bindings(&tags, &cursor, 9);

        assert!(matches!(bindings[0], D1Type::Text("rust")));
        assert!(matches!(bindings[1], D1Type::Null));
        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(bindings[5], D1Type::Integer(9)));
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
        let sql = local_public_statuses_by_tags_legacy_sql(2);

        assert!(sql.contains("lower(text_content) LIKE ?1 OR lower(text_content) LIKE ?2"));
        assert!(sql.contains("?3 IS NULL"));
        assert!(sql.contains("id < ?4"));
        assert!(sql.contains("?5 IS NULL"));
        assert!(sql.contains("id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
    }

    #[test]
    fn local_public_statuses_by_tags_legacy_bindings_keep_sql_slot_order_stable() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());
        let patterns = vec!["%#rust%".to_owned(), "%#masto%".to_owned()];

        let bindings = local_public_statuses_by_tags_legacy_bindings(&patterns, &cursor, 13);

        assert!(matches!(bindings[0], D1Type::Text("%#rust%")));
        assert!(matches!(bindings[1], D1Type::Text("%#masto%")));
        assert!(matches!(bindings[2], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[3], D1Type::Text("status-max")));
        assert!(matches!(bindings[4], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[5], D1Type::Text("status-min")));
        assert!(matches!(bindings[6], D1Type::Integer(13)));
    }

    #[test]
    fn local_public_statuses_by_tags_legacy_bindings_use_null_for_open_cursor_bounds() {
        let cursor = empty_cursor();
        let patterns = vec!["%#rust%".to_owned()];

        let bindings = local_public_statuses_by_tags_legacy_bindings(&patterns, &cursor, 4);

        assert!(matches!(bindings[0], D1Type::Text("%#rust%")));
        assert!(matches!(bindings[1], D1Type::Null));
        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(bindings[5], D1Type::Integer(4)));
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
        let sql = local_public_statuses_by_link_sql(2);

        assert!(sql.contains(
            "(s.text_content LIKE ?1 OR s.content_html LIKE ?1) OR \
             (s.text_content LIKE ?2 OR s.content_html LIKE ?2)"
        ));
        assert!(sql.contains("?3 IS NULL"));
        assert!(sql.contains("s.id < ?4"));
        assert!(sql.contains("?5 IS NULL"));
        assert!(sql.contains("s.id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
    }

    #[test]
    fn local_public_statuses_by_link_bindings_keep_sql_slot_order_stable() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());
        let patterns = vec!["%https://example.test/a%".to_owned()];

        let bindings = local_public_statuses_by_link_bindings(&patterns, &cursor, 15);

        assert!(matches!(
            bindings[0],
            D1Type::Text("%https://example.test/a%")
        ));
        assert!(matches!(bindings[1], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[2], D1Type::Text("status-max")));
        assert!(matches!(bindings[3], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[4], D1Type::Text("status-min")));
        assert!(matches!(bindings[5], D1Type::Integer(15)));
    }

    #[test]
    fn local_public_statuses_by_link_bindings_use_null_for_open_cursor_bounds() {
        let cursor = empty_cursor();
        let patterns = vec!["%https://example.test/a%".to_owned()];

        let bindings = local_public_statuses_by_link_bindings(&patterns, &cursor, 7);

        assert!(matches!(
            bindings[0],
            D1Type::Text("%https://example.test/a%")
        ));
        assert!(matches!(bindings[1], D1Type::Null));
        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(bindings[5], D1Type::Integer(7)));
    }

    #[test]
    fn local_direct_timeline_bindings_keep_sql_slot_order_stable() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());

        let bindings = local_direct_timeline_bindings("viewer", &cursor, 11);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[2], D1Type::Text("status-max")));
        assert!(matches!(bindings[3], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[4], D1Type::Text("status-min")));
        assert!(matches!(bindings[5], D1Type::Integer(11)));
    }

    #[test]
    fn local_direct_timeline_bindings_use_null_for_open_cursor_bounds() {
        let cursor = empty_cursor();
        let bindings = local_direct_timeline_bindings("viewer", &cursor, 5);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Null));
        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(bindings[5], D1Type::Integer(5)));
    }
}
