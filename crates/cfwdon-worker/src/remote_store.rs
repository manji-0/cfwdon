use crate::{
    AppConfig, RemoteActorProfile, RemoteStatusAttachmentRow, build_remote_status_response,
    count_followers_by_actor, delete_remote_status_poll_by_status_id, extract_remote_poll_draft,
    find_account_by_id, find_local_status_by_object_uri, find_remote_actor_by_actor_uri,
    generate_entity_id, insert_remote_status_edit_snapshot, normalize_status_history_entry,
    now_iso_string, quote_target_uri_from_object, remote_quote_state_for_local_target,
    render_status_html, replace_remote_status_attachments, send_remote_status_quote_notification,
    send_remote_status_update_notifications, upsert_remote_status_poll,
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
    #[serde(default)]
    pub(crate) boost_of_uri: Option<String>,
    #[serde(default)]
    pub(crate) quote_of_uri: Option<String>,
    pub(crate) content_html: String,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) sensitive: i32,
    pub(crate) language: Option<String>,
    #[serde(default = "default_remote_quote_state")]
    pub(crate) quote_state: String,
    pub(crate) published_at: String,
}

pub(crate) fn default_remote_quote_state() -> String {
    "accepted".to_owned()
}

pub(crate) fn effective_remote_status_quote_state(status: &RemoteStatusRow) -> &str {
    if status.quote_of_uri.is_none() {
        "accepted"
    } else {
        status.quote_state.as_str()
    }
}

pub(crate) fn remote_status_has_active_quote(status: &RemoteStatusRow) -> bool {
    status.quote_of_uri.is_some() && effective_remote_status_quote_state(status) != "revoked"
}

pub(crate) async fn find_remote_status_by_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<RemoteStatusRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
         FROM remote_statuses
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<RemoteStatusRow>(None)
    .await
}

pub(crate) async fn find_remote_status_raw_object_by_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<serde_json::Value>> {
    #[derive(Deserialize)]
    struct RemoteStatusRawObjectRow {
        raw_object_json: String,
    }

    let status_id = D1Type::Text(status_id);
    let Some(row) = db
        .prepare(
            "SELECT raw_object_json
             FROM remote_statuses
             WHERE id = ?1
             LIMIT 1",
        )
        .bind_refs(&status_id)?
        .first::<RemoteStatusRawObjectRow>(None)
        .await?
    else {
        return Ok(None);
    };

    serde_json::from_str(&row.raw_object_json)
        .map(Some)
        .map_err(|error| Error::RustError(format!("failed to parse remote status object: {error}")))
}

pub(crate) async fn find_remote_status_by_object_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Option<RemoteStatusRow>> {
    let object_uri = D1Type::Text(object_uri);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
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
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
         FROM remote_statuses
         WHERE object_uri = ?1
            OR url = ?1
         LIMIT 1",
    )
    .bind_refs(&value)?
    .first::<RemoteStatusRow>(None)
    .await
}

#[derive(Debug, Deserialize)]
struct RemoteStatusEditStateRow {
    id: String,
    actor_uri: String,
    object_uri: String,
    url: Option<String>,
    in_reply_to_uri: Option<String>,
    boost_of_uri: Option<String>,
    quote_of_uri: Option<String>,
    content_html: String,
    spoiler_text: String,
    visibility: String,
    sensitive: i32,
    language: Option<String>,
    #[serde(default = "default_remote_quote_state")]
    quote_state: String,
    published_at: String,
    raw_object_json: String,
}

impl RemoteStatusEditStateRow {
    fn status_row(&self) -> RemoteStatusRow {
        RemoteStatusRow {
            id: self.id.clone(),
            actor_uri: self.actor_uri.clone(),
            object_uri: self.object_uri.clone(),
            url: self.url.clone(),
            in_reply_to_uri: self.in_reply_to_uri.clone(),
            boost_of_uri: self.boost_of_uri.clone(),
            quote_of_uri: self.quote_of_uri.clone(),
            content_html: self.content_html.clone(),
            spoiler_text: self.spoiler_text.clone(),
            visibility: self.visibility.clone(),
            sensitive: self.sensitive,
            language: self.language.clone(),
            quote_state: self.quote_state.clone(),
            published_at: self.published_at.clone(),
        }
    }
}

async fn find_remote_status_edit_state_by_object_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Option<RemoteStatusEditStateRow>> {
    let object_uri = D1Type::Text(object_uri);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri,
                content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at,
                raw_object_json
         FROM remote_statuses
         WHERE object_uri = ?1
         LIMIT 1",
    )
    .bind_refs(&object_uri)?
    .first::<RemoteStatusEditStateRow>(None)
    .await
}

async fn insert_previous_remote_status_snapshot(
    db: &D1Database,
    config: &AppConfig,
    previous: &RemoteStatusEditStateRow,
    revision_at: &str,
) -> Result<()> {
    let Some(actor) = find_remote_actor_by_actor_uri(db, &previous.actor_uri).await? else {
        return Ok(());
    };
    let response =
        build_remote_status_response(db, config, None, &previous.status_row(), &actor).await?;
    let mut snapshot = serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({}));
    snapshot["created_at"] = serde_json::json!(revision_at);
    let snapshot = normalize_status_history_entry(snapshot);
    let snapshot_json = serde_json::to_string(&snapshot).map_err(|error| {
        Error::RustError(format!(
            "failed to serialize remote status snapshot: {error}"
        ))
    })?;
    insert_remote_status_edit_snapshot(db, &previous.id, &snapshot_json, revision_at).await
}

pub(crate) async fn upsert_remote_status(
    db: &D1Database,
    config: &AppConfig,
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
    let previous = find_remote_status_edit_state_by_object_uri(db, &object_uri).await?;
    let previous_raw_object_json = previous.as_ref().map(|value| value.raw_object_json.clone());
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
    let quote_of_uri = quote_target_uri_from_object(object);
    let quote_state =
        evaluate_remote_quote_state(db, config, actor, quote_of_uri.as_deref()).await?;
    let revision_at = now_iso_string()?;

    if previous
        .as_ref()
        .is_some_and(|existing| existing.raw_object_json != raw_object_json)
    {
        insert_previous_remote_status_snapshot(
            db,
            config,
            previous.as_ref().expect("previous checked above"),
            &revision_at,
        )
        .await?;
    }

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
        D1Type::Null,
        match quote_of_uri.as_deref() {
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
        D1Type::Text(quote_state),
        D1Type::Text(published_at.as_str()),
        D1Type::Text(raw_object_json.as_str()),
        D1Type::Text(revision_at.as_str()),
    ];
    db.prepare(
        "INSERT INTO remote_statuses (
            id,
            actor_uri,
            object_uri,
            url,
            in_reply_to_uri,
            boost_of_uri,
            quote_of_uri,
            content_html,
            spoiler_text,
            visibility,
            sensitive,
            language,
            quote_state,
            published_at,
            raw_object_json,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
            CURRENT_TIMESTAMP,
            ?16
        )
        ON CONFLICT(object_uri) DO UPDATE SET
            actor_uri = excluded.actor_uri,
            url = excluded.url,
            in_reply_to_uri = excluded.in_reply_to_uri,
            boost_of_uri = excluded.boost_of_uri,
            quote_of_uri = excluded.quote_of_uri,
            content_html = excluded.content_html,
            spoiler_text = excluded.spoiler_text,
            visibility = excluded.visibility,
            sensitive = excluded.sensitive,
            language = excluded.language,
            quote_state = CASE
                WHEN remote_statuses.quote_state = 'revoked' THEN remote_statuses.quote_state
                ELSE excluded.quote_state
            END,
            published_at = excluded.published_at,
            raw_object_json = excluded.raw_object_json,
            updated_at = ?16",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let status = find_remote_status_by_object_uri(db, &object_uri)
        .await?
        .ok_or_else(|| Error::RustError("cached remote status could not be reloaded".to_owned()))?;
    replace_remote_status_attachments(
        db,
        &status.id,
        &remote_status_attachments_from_object(&status.id, object),
    )
    .await?;
    if let Some(poll) = extract_remote_poll_draft(object) {
        upsert_remote_status_poll(db, &status.id, &poll).await?;
    } else {
        delete_remote_status_poll_by_status_id(db, &status.id).await?;
    }

    if previous_raw_object_json.is_none() {
        let _ = send_remote_status_quote_notification(
            db,
            config,
            &status.id,
            &status.actor_uri,
            &status.quote_state,
            status.quote_of_uri.as_deref(),
        )
        .await;
    } else if previous_raw_object_json.as_deref() != Some(raw_object_json.as_str()) {
        let _ = send_remote_status_update_notifications(
            db,
            config,
            &status.id,
            &status.actor_uri,
            &status.object_uri,
        )
        .await;
    }

    Ok(())
}

fn remote_status_attachments_from_object(
    status_id: &str,
    object: &serde_json::Value,
) -> Vec<RemoteStatusAttachmentRow> {
    let Some(values) = object
        .get("attachment")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };

    values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let remote_url = attachment_uri(value)?;
            Some(RemoteStatusAttachmentRow {
                id: format!("{status_id}:remote:{index}"),
                status_id: status_id.to_owned(),
                remote_url: remote_url.clone(),
                preview_url: value.get("icon").and_then(attachment_uri),
                content_type: value
                    .get("mediaType")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("application/octet-stream")
                    .to_owned(),
                description: value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                blurhash: value
                    .get("blurhash")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                width: value
                    .get("width")
                    .and_then(serde_json::Value::as_u64)
                    .map(|value| value as u32),
                height: value
                    .get("height")
                    .and_then(serde_json::Value::as_u64)
                    .map(|value| value as u32),
                created_at: now_iso_string().unwrap_or_default(),
            })
        })
        .collect()
}

fn attachment_uri(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(uri) => Some(uri.clone()),
        serde_json::Value::Object(map) => map
            .get("url")
            .and_then(|url| match url {
                serde_json::Value::String(uri) => Some(uri.clone()),
                serde_json::Value::Array(values) => values.iter().find_map(attachment_uri),
                serde_json::Value::Object(_) => {
                    crate::activity_object_id(Some(url)).map(str::to_owned)
                }
                _ => None,
            })
            .or_else(|| {
                map.get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            }),
        serde_json::Value::Array(values) => values.iter().find_map(attachment_uri),
        _ => None,
    }
}

pub(crate) async fn upsert_remote_reblog_status(
    db: &D1Database,
    config: &AppConfig,
    remote_actor: &RemoteActorProfile,
    activity: &serde_json::Value,
) -> Result<()> {
    let object_uri = activity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote announce activity is missing id".to_owned()))?
        .to_owned();
    let boost_of_uri = activity
        .get("object")
        .and_then(|value| crate::activity_object_id(Some(value)))
        .ok_or_else(|| {
            Error::RustError("remote announce activity is missing object id".to_owned())
        })?
        .to_owned();
    let published_at = activity
        .get("published")
        .and_then(serde_json::Value::as_str)
        .or_else(|| activity.get("updated").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
        .unwrap_or(now_iso_string()?);
    let raw_object_json = serde_json::to_string(activity).map_err(|error| {
        Error::RustError(format!(
            "failed to serialize remote announce activity: {error}"
        ))
    })?;
    let status_id = generate_entity_id(16)?;
    let visibility = visibility_from_activitypub_object(activity);
    let quote_of_uri = quote_target_uri_from_object(activity);
    let quote_state =
        evaluate_remote_quote_state(db, config, remote_actor, quote_of_uri.as_deref()).await?;
    let bindings = [
        D1Type::Text(status_id.as_str()),
        D1Type::Text(remote_actor.actor_uri.as_str()),
        D1Type::Text(object_uri.as_str()),
        D1Type::Null,
        D1Type::Null,
        D1Type::Text(boost_of_uri.as_str()),
        match quote_of_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(visibility.as_str()),
        D1Type::Integer(0),
        D1Type::Null,
        D1Type::Text(quote_state),
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
            boost_of_uri,
            quote_of_uri,
            content_html,
            spoiler_text,
            visibility,
            sensitive,
            language,
            quote_state,
            published_at,
            raw_object_json,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(object_uri) DO UPDATE SET
            actor_uri = excluded.actor_uri,
            url = excluded.url,
            in_reply_to_uri = excluded.in_reply_to_uri,
            boost_of_uri = excluded.boost_of_uri,
            quote_of_uri = excluded.quote_of_uri,
            content_html = excluded.content_html,
            spoiler_text = excluded.spoiler_text,
            visibility = excluded.visibility,
            sensitive = excluded.sensitive,
            language = excluded.language,
            quote_state = CASE
                WHEN remote_statuses.quote_state = 'revoked' THEN remote_statuses.quote_state
                ELSE excluded.quote_state
            END,
            published_at = excluded.published_at,
            raw_object_json = excluded.raw_object_json,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn evaluate_remote_quote_state(
    db: &D1Database,
    config: &AppConfig,
    actor: &RemoteActorProfile,
    quote_of_uri: Option<&str>,
) -> Result<&'static str> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok("accepted");
    };
    let Some(status) = find_local_status_by_object_uri(db, config, quote_of_uri).await? else {
        return Ok("accepted");
    };
    let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
        return Ok("accepted");
    };
    let remote_actor_follows_owner =
        count_followers_by_actor(db, &owner.id, &actor.actor_uri).await? > 0;
    let blocked_by_owner = crate::is_blocking_actor(db, &owner.id, &actor.actor_uri).await?;
    Ok(remote_quote_state_for_local_target(
        &status,
        remote_actor_follows_owner,
        blocked_by_owner,
    ))
}

pub(crate) async fn update_remote_status_quote_state(
    db: &D1Database,
    status_id: &str,
    quote_state: &str,
) -> Result<RemoteStatusRow> {
    let bindings = [D1Type::Text(quote_state), D1Type::Text(status_id)];
    db.prepare(
        "UPDATE remote_statuses
         SET quote_state = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    find_remote_status_by_id(db, status_id)
        .await?
        .ok_or_else(|| Error::RustError("remote status not found".to_owned()))
}
