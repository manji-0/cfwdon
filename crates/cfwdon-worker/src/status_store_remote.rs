use crate::{
    D1Database, RemoteActorRow, RemoteStatusRow, ResolvedTimelineCursor, Result, normalize_hashtag,
};
use std::collections::HashSet;
use worker::d1::D1Type;

pub(crate) async fn list_remote_public_timeline_statuses(
    db: &D1Database,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    query_remote_statuses_with_actor(
        db,
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
         WHERE rs.visibility = 'public'
           AND (
                ?1 IS NULL
                OR rs.published_at < ?1
                OR (rs.published_at = ?1 AND rs.id < ?2)
           )
           AND (
                ?3 IS NULL
                OR rs.published_at > ?3
                OR (rs.published_at = ?3 AND rs.id > ?4)
           )
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?5",
        &[
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
        ],
    )
    .await
}

pub(crate) async fn list_remote_home_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    query_remote_statuses_with_actor(
        db,
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
         JOIN follows f
           ON f.target_actor_uri = rs.actor_uri
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE rs.visibility IN ('public', 'unlisted', 'private')
           AND (
                ?2 IS NULL
                OR rs.published_at < ?2
                OR (rs.published_at = ?2 AND rs.id < ?3)
           )
           AND (
                ?4 IS NULL
                OR rs.published_at > ?4
                OR (rs.published_at = ?4 AND rs.id > ?5)
           )
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?6",
        &[
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
        ],
    )
    .await
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

pub(crate) async fn list_remote_public_statuses_by_tags_without_legacy_fallback(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let (rows, _) = list_remote_public_statuses_by_tags_indexed(db, tags, cursor, limit).await?;
    Ok(rows)
}

async fn list_remote_public_statuses_by_tags_indexed(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<(Vec<(RemoteStatusRow, RemoteActorRow)>, Vec<String>)> {
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
         WHERE rs.visibility = 'public'
           AND rs.id IN (
               SELECT h.status_id
               FROM remote_status_hashtags h
               WHERE h.tag IN ({tag_placeholders})
           )
           AND (
                ?{max_timestamp_index} IS NULL
                OR rs.published_at < ?{max_timestamp_index}
                OR (rs.published_at = ?{max_timestamp_index} AND rs.id < ?{max_id_index})
           )
           AND (
                ?{min_timestamp_index} IS NULL
                OR rs.published_at > ?{min_timestamp_index}
                OR (rs.published_at = ?{min_timestamp_index} AND rs.id > ?{min_id_index})
           )
         ORDER BY rs.published_at DESC, rs.id DESC
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

    Ok((
        query_remote_statuses_with_actor(db, &sql, &bindings).await?,
        tags,
    ))
}

async fn list_remote_public_statuses_by_tags_legacy(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let patterns = tags
        .iter()
        .map(|tag| format!("%#{}%", normalize_hashtag(tag)))
        .collect::<Vec<_>>();
    let match_clause = patterns
        .iter()
        .enumerate()
        .map(|(index, _)| format!("lower(rs.content_html) LIKE ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(" OR ");
    let max_timestamp_index = patterns.len() + 1;
    let max_id_index = patterns.len() + 2;
    let min_timestamp_index = patterns.len() + 3;
    let min_id_index = patterns.len() + 4;
    let limit_index = patterns.len() + 5;
    let sql = format!(
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
         WHERE rs.visibility = 'public'
           AND ({match_clause})
           AND (
                ?{max_timestamp_index} IS NULL
                OR rs.published_at < ?{max_timestamp_index}
                OR (rs.published_at = ?{max_timestamp_index} AND rs.id < ?{max_id_index})
           )
           AND (
                ?{min_timestamp_index} IS NULL
                OR rs.published_at > ?{min_timestamp_index}
                OR (rs.published_at = ?{min_timestamp_index} AND rs.id > ?{min_id_index})
           )
         ORDER BY rs.published_at DESC, rs.id DESC
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

    query_remote_statuses_with_actor(db, &sql, &bindings).await
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

    let patterns = urls
        .iter()
        .map(|url| format!("%{url}%"))
        .collect::<Vec<_>>();
    let match_clause = patterns
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let position = index + 1;
            format!(
                "(rs.content_html LIKE ?{position} OR rs.url LIKE ?{position} OR rs.object_uri LIKE ?{position})"
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let cursor_offset = patterns.len();
    let sql = format!(
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
         WHERE rs.visibility = 'public'
           AND ra.discoverable = 1
           AND ({match_clause})
           AND (
                ?{max_timestamp} IS NULL
                OR rs.published_at < ?{max_timestamp}
                OR (rs.published_at = ?{max_timestamp} AND rs.id < ?{max_id})
           )
           AND (
                ?{min_timestamp} IS NULL
                OR rs.published_at > ?{min_timestamp}
                OR (rs.published_at = ?{min_timestamp} AND rs.id > ?{min_id})
           )
         ORDER BY rs.published_at DESC, rs.id DESC
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
    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

pub(crate) async fn list_remote_statuses_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusRow>> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
             FROM remote_statuses
             WHERE actor_uri = ?1
             ORDER BY published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusRow>()
}

pub(crate) async fn list_public_remote_statuses_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
    max_id: Option<&str>,
    min_id: Option<&str>,
    limit: u32,
) -> Result<Vec<RemoteStatusRow>> {
    let bindings = [
        D1Type::Text(actor_uri),
        max_id.map_or(D1Type::Null, D1Type::Text),
        min_id.map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "WITH max_cursor AS (
                SELECT id, published_at FROM remote_statuses WHERE id = ?2 LIMIT 1
             ),
             min_cursor AS (
                SELECT id, published_at FROM remote_statuses WHERE id = ?3 LIMIT 1
             )
             SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
             FROM remote_statuses
             WHERE actor_uri = ?1
               AND visibility IN ('public', 'unlisted')
               AND (
                    ?2 IS NULL
                    OR NOT EXISTS (SELECT 1 FROM max_cursor)
                    OR EXISTS (
                        SELECT 1 FROM max_cursor
                        WHERE remote_statuses.published_at < max_cursor.published_at
                           OR (remote_statuses.published_at = max_cursor.published_at AND remote_statuses.id < max_cursor.id)
                    )
               )
               AND (
                    ?3 IS NULL
                    OR NOT EXISTS (SELECT 1 FROM min_cursor)
                    OR EXISTS (
                        SELECT 1 FROM min_cursor
                        WHERE remote_statuses.published_at > min_cursor.published_at
                           OR (remote_statuses.published_at = min_cursor.published_at AND remote_statuses.id > min_cursor.id)
                    )
               )
             ORDER BY published_at DESC, id DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusRow>()
}

pub(crate) async fn list_direct_remote_replies_by_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    query_remote_statuses_with_actor(
        db,
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
         ORDER BY rs.published_at ASC",
        &[D1Type::Text(object_uri)],
    )
    .await
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
        .map(|value| {
            (
                remote_status_row_from_value(&value),
                RemoteActorRow::from_value(&value),
            )
        })
        .collect())
}

fn remote_status_row_from_value(value: &serde_json::Value) -> RemoteStatusRow {
    RemoteStatusRow {
        id: value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        actor_uri: value
            .get("actor_uri")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        object_uri: value
            .get("object_uri")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        url: value
            .get("url")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned),
        in_reply_to_uri: value
            .get("in_reply_to_uri")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned),
        boost_of_uri: value
            .get("boost_of_uri")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned),
        quote_of_uri: value
            .get("quote_of_uri")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned),
        content_html: value
            .get("content_html")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        spoiler_text: value
            .get("spoiler_text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        visibility: value
            .get("visibility")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        sensitive: value
            .get("sensitive")
            .and_then(|v| v.as_i64())
            .unwrap_or_default() as i32,
        language: value
            .get("language")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned),
        quote_state: value
            .get("quote_state")
            .and_then(|v| v.as_str())
            .unwrap_or("accepted")
            .to_owned(),
        published_at: value
            .get("published_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
    }
}
