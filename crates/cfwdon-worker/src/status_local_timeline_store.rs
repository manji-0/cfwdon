use crate::{D1Database, ResolvedTimelineCursor, Result, StatusRow, normalize_hashtag};
use worker::d1::D1Type;

pub(crate) async fn list_local_home_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
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
            "SELECT DISTINCT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
             FROM statuses s
             LEFT JOIN follows f
               ON f.target_account_id = s.account_id
              AND f.follower_account_id = ?1
              AND f.state = 'accepted'
             WHERE (
                    s.account_id = ?2
                    OR (
                    f.follower_account_id IS NOT NULL
                    AND s.visibility IN ('public', 'unlisted', 'private')
                    )
               )
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
             ORDER BY s.created_at DESC, s.id DESC
             LIMIT ?7",
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
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
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
    let pattern = format!("%#{}%", normalize_hashtag(tag));
    let bindings = [
        D1Type::Text(pattern.as_str()),
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
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE visibility = 'public'
               AND lower(text_content) LIKE ?1
               AND (
                    ?2 IS NULL
                    OR created_at < ?2
                    OR (created_at = ?2 AND id < ?3)
               )
               AND (
                    ?4 IS NULL
                    OR created_at > ?4
                    OR (created_at = ?4 AND id > ?5)
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?6",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

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
        "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
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
            "SELECT DISTINCT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
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
