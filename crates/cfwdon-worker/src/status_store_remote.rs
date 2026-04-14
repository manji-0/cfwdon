use crate::{D1Database, RemoteActorRow, RemoteStatusRow, Result, normalize_hashtag};
use worker::d1::D1Type;

pub(crate) async fn list_remote_public_timeline_statuses(
    db: &D1Database,
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
         WHERE rs.visibility = 'public'
         ORDER BY rs.published_at DESC
         LIMIT ?1",
        &[D1Type::Integer(limit as i32)],
    )
    .await
}

pub(crate) async fn list_remote_home_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
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
         JOIN follows f
           ON f.target_actor_uri = rs.actor_uri
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE rs.visibility IN ('public', 'unlisted', 'private')
         ORDER BY rs.published_at DESC
         LIMIT ?2",
        &[
            D1Type::Text(viewer_account_id),
            D1Type::Integer(limit as i32),
        ],
    )
    .await
}

pub(crate) async fn list_remote_public_statuses_by_tag(
    db: &D1Database,
    tag: &str,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let pattern = format!("%#{}%", normalize_hashtag(tag));
    query_remote_statuses_with_actor(
        db,
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
         WHERE rs.visibility = 'public'
           AND lower(rs.content_html) LIKE ?1
         ORDER BY rs.published_at DESC
         LIMIT ?2",
        &[
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ],
    )
    .await
}

pub(crate) async fn list_remote_statuses_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusRow>> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
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
        published_at: value
            .get("published_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
    }
}
