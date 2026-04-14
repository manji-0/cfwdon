use crate::{RemoteActorRow, RemoteStatusRow, Result, StatusRow};
use worker::D1Database;
use worker::d1::D1Type;

pub(crate) async fn search_local_status_rows(
    db: &D1Database,
    query: &str,
    limit: u32,
    account_id: Option<&str>,
) -> Result<Vec<StatusRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let result = if let Some(account_id) = account_id {
        let bindings = [
            D1Type::Text(account_id),
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE account_id = ?1
               AND (lower(text_content) LIKE ?2 OR lower(spoiler_text) LIKE ?2)
             ORDER BY created_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE lower(text_content) LIKE ?1
                OR lower(spoiler_text) LIKE ?1
             ORDER BY created_at DESC
             LIMIT ?2",
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
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let result = if let Some(actor_uri) = actor_uri {
        let bindings = [
            D1Type::Text(actor_uri),
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE rs.actor_uri = ?1
               AND (lower(rs.content_html) LIKE ?2 OR lower(rs.spoiler_text) LIKE ?2)
             ORDER BY rs.published_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE lower(rs.content_html) LIKE ?1
                OR lower(rs.spoiler_text) LIKE ?1
             ORDER BY rs.published_at DESC
             LIMIT ?2",
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
