use crate::{
    RemoteActorProfile, delete_remote_status_poll_by_status_id, extract_remote_poll_draft,
    generate_entity_id, render_status_html, upsert_remote_status_poll,
    visibility_from_activitypub_object,
};
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteStatusRow {
    pub(crate) id: String,
    pub(crate) actor_uri: String,
    pub(crate) object_uri: String,
    pub(crate) url: Option<String>,
    pub(crate) in_reply_to_uri: Option<String>,
    pub(crate) content_html: String,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    pub(crate) published_at: String,
}

pub(crate) async fn find_remote_status_by_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<RemoteStatusRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
         FROM remote_statuses
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<RemoteStatusRow>(None)
    .await
}

pub(crate) async fn find_remote_status_by_object_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Option<RemoteStatusRow>> {
    let object_uri = D1Type::Text(object_uri);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
         FROM remote_statuses
         WHERE object_uri = ?1
         LIMIT 1",
    )
    .bind_refs(&object_uri)?
    .first::<RemoteStatusRow>(None)
    .await
}

pub(crate) async fn find_remote_status_by_url_or_object_uri(
    db: &D1Database,
    value: &str,
) -> Result<Option<RemoteStatusRow>> {
    let value = D1Type::Text(value);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
         FROM remote_statuses
         WHERE object_uri = ?1
            OR url = ?1
         LIMIT 1",
    )
    .bind_refs(&value)?
    .first::<RemoteStatusRow>(None)
    .await
}

pub(crate) async fn upsert_remote_status(
    db: &D1Database,
    actor: &RemoteActorProfile,
    object: &serde_json::Value,
) -> Result<()> {
    let object_uri = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote status object is missing id".to_owned()))?
        .to_owned();
    let raw_object_json = serde_json::to_string(object).map_err(|error| {
        Error::RustError(format!("failed to serialize remote status object: {error}"))
    })?;
    let visibility = visibility_from_activitypub_object(object);
    let content_html = object
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            object
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(render_status_html)
        })
        .unwrap_or_default();
    let spoiler_text = object
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let sensitive = object
        .get("sensitive")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let published_at = object
        .get("published")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| {
            object
                .get("updated")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        })
        .to_owned();
    let language = object
        .get("contentMap")
        .and_then(serde_json::Value::as_object)
        .and_then(|map| map.keys().next().cloned());
    let status_id = generate_entity_id(16)?;

    let bindings = [
        D1Type::Text(status_id.as_str()),
        D1Type::Text(actor.actor_uri.as_str()),
        D1Type::Text(object_uri.as_str()),
        match object.get("url").and_then(serde_json::Value::as_str) {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match object.get("inReplyTo").and_then(serde_json::Value::as_str) {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(content_html.as_str()),
        D1Type::Text(spoiler_text.as_str()),
        D1Type::Text(visibility.as_str()),
        D1Type::Integer(if sensitive { 1 } else { 0 }),
        match language.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(published_at.as_str()),
        D1Type::Text(raw_object_json.as_str()),
    ];
    db.prepare(
        "INSERT INTO remote_statuses (
            id,
            actor_uri,
            object_uri,
            url,
            in_reply_to_uri,
            content_html,
            spoiler_text,
            visibility,
            sensitive,
            language,
            published_at,
            raw_object_json,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(object_uri) DO UPDATE SET
            actor_uri = excluded.actor_uri,
            url = excluded.url,
            in_reply_to_uri = excluded.in_reply_to_uri,
            content_html = excluded.content_html,
            spoiler_text = excluded.spoiler_text,
            visibility = excluded.visibility,
            sensitive = excluded.sensitive,
            language = excluded.language,
            published_at = excluded.published_at,
            raw_object_json = excluded.raw_object_json,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let status = find_remote_status_by_object_uri(db, &object_uri)
        .await?
        .ok_or_else(|| Error::RustError("cached remote status could not be reloaded".to_owned()))?;
    if let Some(poll) = extract_remote_poll_draft(object) {
        upsert_remote_status_poll(db, &status.id, &poll).await?;
    } else {
        delete_remote_status_poll_by_status_id(db, &status.id).await?;
    }

    Ok(())
}
