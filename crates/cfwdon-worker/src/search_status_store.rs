use crate::{RemoteActorRow, RemoteStatusRow, Result, StatusRow};
use worker::D1Database;
use worker::d1::D1Type;

pub(crate) async fn search_local_status_rows(
    db: &D1Database,
    query: &str,
    limit: u32,
    account_id: Option<&str>,
    max_id: Option<&str>,
    max_timestamp: Option<&str>,
    min_id: Option<&str>,
    min_timestamp: Option<&str>,
) -> Result<Vec<StatusRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let result = if let Some(account_id) = account_id {
        let bindings = [
            D1Type::Text(account_id),
            D1Type::Text(pattern.as_str()),
            match max_timestamp {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match max_id {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match min_timestamp {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match min_id {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE account_id = ?1
               AND (lower(text_content) LIKE ?2 OR lower(spoiler_text) LIKE ?2)
               AND (?3 IS NULL
                    OR created_at < ?3
                    OR (created_at = ?3 AND id < ?4))
               AND (?5 IS NULL
                    OR created_at > ?5
                    OR (created_at = ?5 AND id > ?6))
             ORDER BY created_at DESC, id DESC
             LIMIT ?7",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            match max_timestamp {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match max_id {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match min_timestamp {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match min_id {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE (lower(text_content) LIKE ?1
                OR lower(spoiler_text) LIKE ?1)
               AND (?2 IS NULL
                    OR created_at < ?2
                    OR (created_at = ?2 AND id < ?3))
               AND (?4 IS NULL
                    OR created_at > ?4
                    OR (created_at = ?4 AND id > ?5))
             ORDER BY created_at DESC, id DESC
             LIMIT ?6",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    result.results::<StatusRow>()
}

pub(crate) async fn search_remote_status_rows(
    db: &D1Database,
    query: &str,
    limit: u32,
    actor_uri: Option<&str>,
    max_id: Option<&str>,
    max_timestamp: Option<&str>,
    min_id: Option<&str>,
    min_timestamp: Option<&str>,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let result = if let Some(actor_uri) = actor_uri {
        let bindings = [
            D1Type::Text(actor_uri),
            D1Type::Text(pattern.as_str()),
            match max_timestamp {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match max_id {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match min_timestamp {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match min_id {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
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
             WHERE rs.actor_uri = ?1
               AND (lower(rs.content_html) LIKE ?2 OR lower(rs.spoiler_text) LIKE ?2)
               AND (?3 IS NULL
                    OR rs.published_at < ?3
                    OR (rs.published_at = ?3 AND rs.id < ?4))
               AND (?5 IS NULL
                    OR rs.published_at > ?5
                    OR (rs.published_at = ?5 AND rs.id > ?6))
             ORDER BY rs.published_at DESC, rs.id DESC
             LIMIT ?7",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            match max_timestamp {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match max_id {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match min_timestamp {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            match min_id {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
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
             WHERE (lower(rs.content_html) LIKE ?1
                OR lower(rs.spoiler_text) LIKE ?1)
               AND (?2 IS NULL
                    OR rs.published_at < ?2
                    OR (rs.published_at = ?2 AND rs.id < ?3))
               AND (?4 IS NULL
                    OR rs.published_at > ?4
                    OR (rs.published_at = ?4 AND rs.id > ?5))
             ORDER BY rs.published_at DESC, rs.id DESC
             LIMIT ?6",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    let values = result.results::<serde_json::Value>()?;
    let mut rows = Vec::with_capacity(values.len());
    for value in values {
        rows.push((
            RemoteStatusRow {
                id: value
                    .get("id")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                actor_uri: value
                    .get("actor_uri")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                object_uri: value
                    .get("object_uri")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                url: value
                    .get("url")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                in_reply_to_uri: value
                    .get("in_reply_to_uri")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                boost_of_uri: value
                    .get("boost_of_uri")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                quote_of_uri: value
                    .get("quote_of_uri")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                content_html: value
                    .get("content_html")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                spoiler_text: value
                    .get("spoiler_text")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                visibility: value
                    .get("visibility")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                sensitive: value
                    .get("sensitive")
                    .and_then(|field| field.as_i64())
                    .unwrap_or_default() as i32,
                language: value
                    .get("language")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                quote_state: value
                    .get("quote_state")
                    .and_then(|field| field.as_str())
                    .unwrap_or("accepted")
                    .to_owned(),
                published_at: value
                    .get("published_at")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            },
            RemoteActorRow::from_value(&value),
        ));
    }

    Ok(rows)
}
