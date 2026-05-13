use crate::{D1Database, ResolvedTimelineCursor, Result, StatusRow, normalize_hashtag};
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

    let bindings = [
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
    ];
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
    let bindings = [
        D1Type::Text(viewer_account_id),
        D1Type::Text(viewer_account_id),
        D1Type::Text(min_timestamp),
        min_id.map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ];
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

pub(crate) async fn list_local_public_timeline_statuses(
    db: &D1Database,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [
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
    ];
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
    let mut seen = HashSet::new();
    let tags = tags
        .iter()
        .map(|tag| normalize_hashtag(tag))
        .filter(|tag| !tag.is_empty() && seen.insert(tag.clone()))
        .collect::<Vec<_>>();
    if tags.is_empty() {
        return Ok((Vec::new(), tags));
    }

    let tag_placeholders = (1..=tags.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let max_timestamp_index = tags.len() + 1;
    let max_id_index = tags.len() + 2;
    let min_timestamp_index = tags.len() + 3;
    let min_id_index = tags.len() + 4;
    let limit_index = tags.len() + 5;
    let sql = format!(
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
    );
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
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok((result.results::<StatusRow>()?, tags))
}

async fn list_local_public_statuses_by_tags_legacy(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let patterns = tags
        .iter()
        .map(|tag| format!("%#{}%", normalize_hashtag(tag)))
        .collect::<Vec<_>>();
    let match_clause = patterns
        .iter()
        .enumerate()
        .map(|(index, _)| format!("lower(text_content) LIKE ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(" OR ");
    let max_timestamp_index = patterns.len() + 1;
    let max_id_index = patterns.len() + 2;
    let min_timestamp_index = patterns.len() + 3;
    let min_id_index = patterns.len() + 4;
    let limit_index = patterns.len() + 5;
    let sql = format!(
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
    );
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
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    result.results::<StatusRow>()
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

    let patterns = urls
        .iter()
        .map(|url| format!("%{url}%"))
        .collect::<Vec<_>>();
    let match_clause = patterns
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let position = index + 1;
            format!("(s.text_content LIKE ?{position} OR s.content_html LIKE ?{position})")
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let cursor_offset = patterns.len();
    let sql = format!(
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
    );
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
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    result.results::<StatusRow>()
}

pub(crate) async fn list_local_direct_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [
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
    ];
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
