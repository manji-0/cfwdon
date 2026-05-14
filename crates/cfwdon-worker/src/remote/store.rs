use crate::{
    AppConfig, RemoteActorProfile, RemoteStatusAttachmentRow, build_remote_status_response,
    count_followers_by_actor, delete_remote_status_poll_by_status_id, extract_remote_poll_draft,
    find_account_by_id, find_local_status_by_object_uri, find_remote_actor_by_actor_uri,
    generate_entity_id, insert_remote_status_edit_snapshot, normalize_status_history_entry,
    now_iso_string, quote_target_uri_from_object, remote_quote_state_for_local_target,
    render_status_html, replace_remote_status_attachments, replace_remote_status_hashtags,
    send_remote_status_quote_notification, send_remote_status_update_notifications,
    upsert_remote_status_poll, visibility_from_activitypub_object,
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
    let bindings = remote_status_id_bindings(status_id);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
         FROM remote_statuses
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
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

    let bindings = remote_status_id_bindings(status_id);
    let Some(row) = db
        .prepare(
            "SELECT raw_object_json
             FROM remote_statuses
             WHERE id = ?1
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
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
    let bindings = remote_status_object_uri_bindings(object_uri);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
         FROM remote_statuses
         WHERE object_uri = ?1
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<RemoteStatusRow>(None)
    .await
}

pub(crate) async fn find_remote_status_by_url_or_object_uri(
    db: &D1Database,
    value: &str,
) -> Result<Option<RemoteStatusRow>> {
    let bindings = remote_status_lookup_value_bindings(value);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
         FROM remote_statuses
         WHERE object_uri = ?1
            OR url = ?1
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
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

fn remote_status_content_html(object: &serde_json::Value) -> String {
    object
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            object
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(render_status_html)
        })
        .unwrap_or_default()
}

fn remote_status_published_at(object: &serde_json::Value) -> String {
    object
        .get("published")
        .and_then(serde_json::Value::as_str)
        .or_else(|| object.get("updated").and_then(serde_json::Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn remote_status_language(object: &serde_json::Value) -> Option<String> {
    object
        .get("contentMap")
        .and_then(serde_json::Value::as_object)
        .and_then(|map| map.keys().next().cloned())
}

fn optional_text_binding(value: Option<&str>) -> D1Type<'_> {
    match value {
        Some(value) => D1Type::Text(value),
        None => D1Type::Null,
    }
}

fn bool_binding(value: bool) -> D1Type<'static> {
    D1Type::Integer(i32::from(value))
}

fn remote_status_id_bindings(status_id: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(status_id)]
}

fn remote_status_object_uri_bindings(object_uri: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(object_uri)]
}

fn remote_status_lookup_value_bindings(value: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(value)]
}

fn remote_status_object_uri(object: &serde_json::Value) -> Result<&str> {
    object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote status object is missing id".to_owned()))
}

struct RemoteStatusUpsertDraft {
    status_id: String,
    actor_uri: String,
    object_uri: String,
    url: Option<String>,
    in_reply_to_uri: Option<String>,
    quote_of_uri: Option<String>,
    content_html: String,
    spoiler_text: String,
    visibility: String,
    sensitive: bool,
    language: Option<String>,
    quote_state: &'static str,
    published_at: String,
    raw_object_json: String,
    revision_at: String,
}

struct RemoteReblogUpsertDraft {
    status_id: String,
    actor_uri: String,
    object_uri: String,
    boost_of_uri: String,
    quote_of_uri: Option<String>,
    visibility: String,
    quote_state: &'static str,
    published_at: String,
    raw_object_json: String,
}

fn serialize_remote_store_json(value: &serde_json::Value, label: &str) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|error| Error::RustError(format!("failed to serialize {label}: {error}")))
}

fn serialize_remote_status_object_json(object: &serde_json::Value) -> Result<String> {
    serialize_remote_store_json(object, "remote status object")
}

fn serialize_remote_reblog_activity_json(activity: &serde_json::Value) -> Result<String> {
    serialize_remote_store_json(activity, "remote announce activity")
}

fn serialize_remote_status_snapshot_json(snapshot: &serde_json::Value) -> Result<String> {
    serialize_remote_store_json(snapshot, "remote status snapshot")
}

fn remote_status_upsert_draft(
    actor: &RemoteActorProfile,
    object: &serde_json::Value,
    status_id: String,
    quote_state: &'static str,
    revision_at: String,
) -> Result<RemoteStatusUpsertDraft> {
    let object_uri = remote_status_object_uri(object)?.to_owned();
    let raw_object_json = serialize_remote_status_object_json(object)?;
    let quote_of_uri = quote_target_uri_from_object(object);

    Ok(RemoteStatusUpsertDraft {
        status_id,
        actor_uri: actor.actor_uri.clone(),
        object_uri,
        url: object
            .get("url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        in_reply_to_uri: object
            .get("inReplyTo")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        quote_of_uri,
        content_html: remote_status_content_html(object),
        spoiler_text: object
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        visibility: visibility_from_activitypub_object(object),
        sensitive: object
            .get("sensitive")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        language: remote_status_language(object),
        quote_state,
        published_at: remote_status_published_at(object),
        raw_object_json,
        revision_at,
    })
}

fn remote_reblog_activity_uri(activity: &serde_json::Value) -> Result<&str> {
    activity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote announce activity is missing id".to_owned()))
}

fn remote_reblog_boost_of_uri(activity: &serde_json::Value) -> Result<&str> {
    activity
        .get("object")
        .and_then(|value| crate::activity_object_id(Some(value)))
        .ok_or_else(|| Error::RustError("remote announce activity is missing object id".to_owned()))
}

fn remote_reblog_published_at(activity: &serde_json::Value, fallback: String) -> String {
    activity
        .get("published")
        .and_then(serde_json::Value::as_str)
        .or_else(|| activity.get("updated").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
        .unwrap_or(fallback)
}

fn remote_reblog_upsert_draft(
    actor: &RemoteActorProfile,
    activity: &serde_json::Value,
    status_id: String,
    quote_of_uri: Option<String>,
    quote_state: &'static str,
    fallback_published_at: String,
) -> Result<RemoteReblogUpsertDraft> {
    let object_uri = remote_reblog_activity_uri(activity)?.to_owned();
    let boost_of_uri = remote_reblog_boost_of_uri(activity)?.to_owned();
    let raw_object_json = serialize_remote_reblog_activity_json(activity)?;

    Ok(RemoteReblogUpsertDraft {
        status_id,
        actor_uri: actor.actor_uri.clone(),
        object_uri,
        boost_of_uri,
        quote_of_uri,
        visibility: visibility_from_activitypub_object(activity),
        quote_state,
        published_at: remote_reblog_published_at(activity, fallback_published_at),
        raw_object_json,
    })
}

async fn find_remote_status_edit_state_by_object_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Option<RemoteStatusEditStateRow>> {
    let bindings = remote_status_object_uri_bindings(object_uri);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri,
                content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at,
                raw_object_json
         FROM remote_statuses
         WHERE object_uri = ?1
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
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
    let snapshot_json = serialize_remote_status_snapshot_json(&snapshot)?;
    insert_remote_status_edit_snapshot(db, &previous.id, &snapshot_json, revision_at).await
}

async fn upsert_remote_status_draft(
    db: &D1Database,
    draft: &RemoteStatusUpsertDraft,
) -> Result<()> {
    let bindings = remote_status_upsert_bindings(draft);
    db.prepare(remote_status_upsert_sql())
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    Ok(())
}

fn remote_status_upsert_sql() -> &'static str {
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
        updated_at = ?16"
}

fn remote_status_upsert_bindings(draft: &RemoteStatusUpsertDraft) -> [D1Type<'_>; 16] {
    [
        D1Type::Text(draft.status_id.as_str()),
        D1Type::Text(draft.actor_uri.as_str()),
        D1Type::Text(draft.object_uri.as_str()),
        optional_text_binding(draft.url.as_deref()),
        optional_text_binding(draft.in_reply_to_uri.as_deref()),
        D1Type::Null,
        optional_text_binding(draft.quote_of_uri.as_deref()),
        D1Type::Text(draft.content_html.as_str()),
        D1Type::Text(draft.spoiler_text.as_str()),
        D1Type::Text(draft.visibility.as_str()),
        bool_binding(draft.sensitive),
        optional_text_binding(draft.language.as_deref()),
        D1Type::Text(draft.quote_state),
        D1Type::Text(draft.published_at.as_str()),
        D1Type::Text(draft.raw_object_json.as_str()),
        D1Type::Text(draft.revision_at.as_str()),
    ]
}

async fn insert_previous_remote_status_snapshot_if_changed(
    db: &D1Database,
    config: &AppConfig,
    previous: Option<&RemoteStatusEditStateRow>,
    draft: &RemoteStatusUpsertDraft,
) -> Result<()> {
    if let Some(previous) =
        previous.filter(|existing| existing.raw_object_json != draft.raw_object_json)
    {
        insert_previous_remote_status_snapshot(db, config, previous, &draft.revision_at).await?;
    }

    Ok(())
}

async fn reload_upserted_remote_status(
    db: &D1Database,
    draft: &RemoteStatusUpsertDraft,
) -> Result<RemoteStatusRow> {
    find_remote_status_by_object_uri(db, &draft.object_uri)
        .await?
        .ok_or_else(|| Error::RustError("cached remote status could not be reloaded".to_owned()))
}

async fn replace_remote_status_dependents(
    db: &D1Database,
    status: &RemoteStatusRow,
    object: &serde_json::Value,
) -> Result<()> {
    replace_remote_status_hashtags(
        db,
        &status.id,
        &status.actor_uri,
        &status.published_at,
        &status.content_html,
    )
    .await?;
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

    Ok(())
}

async fn send_remote_status_change_notifications(
    db: &D1Database,
    config: &AppConfig,
    previous_raw_object_json: Option<&str>,
    status: &RemoteStatusRow,
    draft: &RemoteStatusUpsertDraft,
) {
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
    } else if previous_raw_object_json != Some(draft.raw_object_json.as_str()) {
        let _ = send_remote_status_update_notifications(
            db,
            config,
            &status.id,
            &status.actor_uri,
            &status.object_uri,
        )
        .await;
    }
}

pub(crate) async fn upsert_remote_status(
    db: &D1Database,
    config: &AppConfig,
    actor: &RemoteActorProfile,
    object: &serde_json::Value,
) -> Result<()> {
    let object_uri = remote_status_object_uri(object)?;
    let previous = find_remote_status_edit_state_by_object_uri(db, object_uri).await?;
    let quote_of_uri = quote_target_uri_from_object(object);
    let quote_state =
        evaluate_remote_quote_state(db, config, actor, quote_of_uri.as_deref()).await?;
    let revision_at = now_iso_string()?;
    let draft = remote_status_upsert_draft(
        actor,
        object,
        generate_entity_id(16)?,
        quote_state,
        revision_at,
    )?;

    insert_previous_remote_status_snapshot_if_changed(db, config, previous.as_ref(), &draft)
        .await?;
    upsert_remote_status_draft(db, &draft).await?;

    let status = reload_upserted_remote_status(db, &draft).await?;
    replace_remote_status_dependents(db, &status, object).await?;
    send_remote_status_change_notifications(
        db,
        config,
        previous
            .as_ref()
            .map(|value| value.raw_object_json.as_str()),
        &status,
        &draft,
    )
    .await;

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
    let quote_of_uri = quote_target_uri_from_object(activity);
    let quote_state =
        evaluate_remote_quote_state(db, config, remote_actor, quote_of_uri.as_deref()).await?;
    let draft = remote_reblog_upsert_draft(
        remote_actor,
        activity,
        generate_entity_id(16)?,
        quote_of_uri,
        quote_state,
        now_iso_string()?,
    )?;

    upsert_remote_reblog_status_draft(db, &draft).await
}

async fn upsert_remote_reblog_status_draft(
    db: &D1Database,
    draft: &RemoteReblogUpsertDraft,
) -> Result<()> {
    let bindings = remote_reblog_upsert_bindings(draft);
    db.prepare(remote_reblog_upsert_sql())
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    Ok(())
}

fn remote_reblog_upsert_sql() -> &'static str {
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
        updated_at = CURRENT_TIMESTAMP"
}

fn remote_reblog_upsert_bindings(draft: &RemoteReblogUpsertDraft) -> [D1Type<'_>; 15] {
    let bindings = [
        D1Type::Text(draft.status_id.as_str()),
        D1Type::Text(draft.actor_uri.as_str()),
        D1Type::Text(draft.object_uri.as_str()),
        D1Type::Null,
        D1Type::Null,
        D1Type::Text(draft.boost_of_uri.as_str()),
        optional_text_binding(draft.quote_of_uri.as_deref()),
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(draft.visibility.as_str()),
        D1Type::Integer(0),
        D1Type::Null,
        D1Type::Text(draft.quote_state),
        D1Type::Text(draft.published_at.as_str()),
        D1Type::Text(draft.raw_object_json.as_str()),
    ];
    bindings
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
    let bindings = remote_status_quote_state_update_bindings(quote_state, status_id);
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

fn remote_status_quote_state_update_bindings<'a>(
    quote_state: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 2] {
    [D1Type::Text(quote_state), D1Type::Text(status_id)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn remote_actor_profile_fixture() -> RemoteActorProfile {
        RemoteActorProfile {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            inbox_uri: "https://remote.example/users/alice/inbox".to_owned(),
            shared_inbox_uri: Some("https://remote.example/inbox".to_owned()),
            public_key_id: "https://remote.example/users/alice#main-key".to_owned(),
            public_key_pem: "pem".to_owned(),
            display_name: "Alice".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@alice".to_owned()),
            avatar_url: None,
            header_url: None,
        }
    }

    #[test]
    fn remote_status_object_uri_requires_id() {
        let error = remote_status_object_uri(&json!({})).unwrap_err();

        assert!(error.to_string().contains("missing id"));
    }

    #[test]
    fn remote_status_id_bindings_keep_sql_slot_order_stable() {
        let bindings = remote_status_id_bindings("remote-status-id");

        assert!(matches!(bindings[0], D1Type::Text("remote-status-id")));
    }

    #[test]
    fn remote_status_object_uri_bindings_keep_sql_slot_order_stable() {
        let bindings =
            remote_status_object_uri_bindings("https://remote.example/users/alice/statuses/1");

        assert!(matches!(
            bindings[0],
            D1Type::Text("https://remote.example/users/alice/statuses/1")
        ));
    }

    #[test]
    fn remote_status_lookup_value_bindings_keep_sql_slot_order_stable() {
        let bindings = remote_status_lookup_value_bindings("https://remote.example/@alice/1");

        assert!(matches!(
            bindings[0],
            D1Type::Text("https://remote.example/@alice/1")
        ));
    }

    #[test]
    fn remote_status_quote_state_update_bindings_keep_sql_slot_order_stable() {
        let bindings = remote_status_quote_state_update_bindings("revoked", "remote-status-id");

        assert!(matches!(bindings[0], D1Type::Text("revoked")));
        assert!(matches!(bindings[1], D1Type::Text("remote-status-id")));
    }

    #[test]
    fn remote_status_upsert_sql_preserves_revoked_quote_state() {
        let sql = remote_status_upsert_sql();

        assert!(sql.contains("ON CONFLICT(object_uri) DO UPDATE SET"));
        assert!(sql.contains("WHEN remote_statuses.quote_state = 'revoked'"));
        assert!(sql.contains("updated_at = ?16"));
    }

    #[test]
    fn remote_reblog_upsert_sql_preserves_revoked_quote_state() {
        let sql = remote_reblog_upsert_sql();

        assert!(sql.contains("ON CONFLICT(object_uri) DO UPDATE SET"));
        assert!(sql.contains("WHEN remote_statuses.quote_state = 'revoked'"));
        assert!(sql.contains("updated_at = CURRENT_TIMESTAMP"));
    }

    #[test]
    fn serialize_remote_status_object_json_preserves_status_payload() {
        let object = json!({
            "type": "Note",
            "content": "<p>Hello</p>",
            "sensitive": false
        });

        let json = serialize_remote_status_object_json(&object).unwrap();

        assert_eq!(
            json,
            "{\"content\":\"<p>Hello</p>\",\"sensitive\":false,\"type\":\"Note\"}"
        );
    }

    #[test]
    fn serialize_remote_reblog_activity_json_preserves_announce_payload() {
        let activity = json!({
            "type": "Announce",
            "object": "https://remote.example/users/bob/statuses/9"
        });

        let json = serialize_remote_reblog_activity_json(&activity).unwrap();

        assert_eq!(
            json,
            "{\"object\":\"https://remote.example/users/bob/statuses/9\",\"type\":\"Announce\"}"
        );
    }

    #[test]
    fn serialize_remote_status_snapshot_json_preserves_history_payload() {
        let snapshot = json!({
            "created_at": "2026-05-10T01:02:03Z",
            "content": "<p>Before</p>"
        });

        let json = serialize_remote_status_snapshot_json(&snapshot).unwrap();

        assert_eq!(
            json,
            "{\"content\":\"<p>Before</p>\",\"created_at\":\"2026-05-10T01:02:03Z\"}"
        );
    }

    #[test]
    fn remote_status_upsert_draft_extracts_storage_fields() {
        let actor = remote_actor_profile_fixture();
        let object = json!({
            "id": "https://remote.example/users/alice/statuses/1",
            "url": "https://remote.example/@alice/1",
            "inReplyTo": "https://remote.example/users/bob/statuses/9",
            "content": "<p>Hello</p>",
            "summary": "spoiler",
            "sensitive": true,
            "published": "2026-05-10T01:02:03Z",
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "contentMap": {
                "ja": "<p>Hello</p>"
            }
        });

        let draft = remote_status_upsert_draft(
            &actor,
            &object,
            "remote-status-id".to_owned(),
            "accepted",
            "revision-time".to_owned(),
        )
        .unwrap();

        assert_eq!(draft.actor_uri, actor.actor_uri);
        assert_eq!(
            draft.object_uri,
            "https://remote.example/users/alice/statuses/1"
        );
        assert_eq!(
            draft.url.as_deref(),
            Some("https://remote.example/@alice/1")
        );
        assert_eq!(
            draft.in_reply_to_uri.as_deref(),
            Some("https://remote.example/users/bob/statuses/9")
        );
        assert_eq!(draft.content_html, "<p>Hello</p>");
        assert_eq!(draft.spoiler_text, "spoiler");
        assert_eq!(draft.visibility, "public");
        assert!(draft.sensitive);
        assert_eq!(draft.language.as_deref(), Some("ja"));
        assert_eq!(draft.quote_state, "accepted");
        assert_eq!(draft.published_at, "2026-05-10T01:02:03Z");
        assert_eq!(draft.revision_at, "revision-time");
        assert_eq!(draft.status_id, "remote-status-id");
    }

    #[test]
    fn remote_status_upsert_bindings_keep_sql_slot_order_stable() {
        let draft = RemoteStatusUpsertDraft {
            status_id: "remote-status-id".to_owned(),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: "https://remote.example/users/alice/statuses/1".to_owned(),
            url: Some("https://remote.example/@alice/1".to_owned()),
            in_reply_to_uri: Some("https://remote.example/users/bob/statuses/9".to_owned()),
            quote_of_uri: Some("https://local.example/users/alice/statuses/2".to_owned()),
            content_html: "<p>Hello</p>".to_owned(),
            spoiler_text: "spoiler".to_owned(),
            visibility: "public".to_owned(),
            sensitive: true,
            language: Some("ja".to_owned()),
            quote_state: "accepted",
            published_at: "2026-05-10T01:02:03Z".to_owned(),
            raw_object_json: "{\"type\":\"Note\"}".to_owned(),
            revision_at: "revision-time".to_owned(),
        };
        let bindings = remote_status_upsert_bindings(&draft);

        assert!(matches!(bindings[0], D1Type::Text("remote-status-id")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("https://remote.example/users/alice")
        ));
        assert!(matches!(
            bindings[2],
            D1Type::Text("https://remote.example/users/alice/statuses/1")
        ));
        assert!(matches!(
            bindings[3],
            D1Type::Text("https://remote.example/@alice/1")
        ));
        assert!(matches!(
            bindings[4],
            D1Type::Text("https://remote.example/users/bob/statuses/9")
        ));
        assert!(matches!(bindings[5], D1Type::Null));
        assert!(matches!(
            bindings[6],
            D1Type::Text("https://local.example/users/alice/statuses/2")
        ));
        assert!(matches!(bindings[7], D1Type::Text("<p>Hello</p>")));
        assert!(matches!(bindings[8], D1Type::Text("spoiler")));
        assert!(matches!(bindings[9], D1Type::Text("public")));
        assert!(matches!(bindings[10], D1Type::Integer(1)));
        assert!(matches!(bindings[11], D1Type::Text("ja")));
        assert!(matches!(bindings[12], D1Type::Text("accepted")));
        assert!(matches!(bindings[13], D1Type::Text("2026-05-10T01:02:03Z")));
        assert!(matches!(bindings[14], D1Type::Text("{\"type\":\"Note\"}")));
        assert!(matches!(bindings[15], D1Type::Text("revision-time")));
    }

    #[test]
    fn remote_reblog_upsert_draft_extracts_storage_fields() {
        let actor = remote_actor_profile_fixture();
        let activity = json!({
            "id": "https://remote.example/users/alice/activities/announce/1",
            "type": "Announce",
            "object": {
                "id": "https://remote.example/users/bob/statuses/9"
            },
            "published": "2026-05-11T01:02:03Z",
            "to": ["https://www.w3.org/ns/activitystreams#Public"]
        });

        let draft = remote_reblog_upsert_draft(
            &actor,
            &activity,
            "remote-reblog-id".to_owned(),
            Some("https://local.example/users/alice/statuses/2".to_owned()),
            "accepted",
            "fallback-time".to_owned(),
        )
        .unwrap();

        assert_eq!(draft.status_id, "remote-reblog-id");
        assert_eq!(draft.actor_uri, actor.actor_uri);
        assert_eq!(
            draft.object_uri,
            "https://remote.example/users/alice/activities/announce/1"
        );
        assert_eq!(
            draft.boost_of_uri,
            "https://remote.example/users/bob/statuses/9"
        );
        assert_eq!(
            draft.quote_of_uri.as_deref(),
            Some("https://local.example/users/alice/statuses/2")
        );
        assert_eq!(draft.visibility, "public");
        assert_eq!(draft.quote_state, "accepted");
        assert_eq!(draft.published_at, "2026-05-11T01:02:03Z");
        assert!(draft.raw_object_json.contains("\"Announce\""));
    }

    #[test]
    fn remote_reblog_upsert_draft_uses_fallback_published_at() {
        let actor = remote_actor_profile_fixture();
        let activity = json!({
            "id": "https://remote.example/users/alice/activities/announce/1",
            "type": "Announce",
            "object": "https://remote.example/users/bob/statuses/9"
        });

        let draft = remote_reblog_upsert_draft(
            &actor,
            &activity,
            "remote-reblog-id".to_owned(),
            None,
            "accepted",
            "fallback-time".to_owned(),
        )
        .unwrap();

        assert_eq!(draft.published_at, "fallback-time");
    }

    #[test]
    fn remote_reblog_upsert_bindings_keep_sql_slot_order_stable() {
        let draft = RemoteReblogUpsertDraft {
            status_id: "remote-reblog-id".to_owned(),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: "https://remote.example/users/alice/activities/announce/1".to_owned(),
            boost_of_uri: "https://remote.example/users/bob/statuses/9".to_owned(),
            quote_of_uri: Some("https://local.example/users/alice/statuses/2".to_owned()),
            visibility: "unlisted".to_owned(),
            quote_state: "accepted",
            published_at: "2026-05-11T01:02:03Z".to_owned(),
            raw_object_json: "{\"type\":\"Announce\"}".to_owned(),
        };
        let bindings = remote_reblog_upsert_bindings(&draft);

        assert!(matches!(bindings[0], D1Type::Text("remote-reblog-id")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("https://remote.example/users/alice")
        ));
        assert!(matches!(
            bindings[2],
            D1Type::Text("https://remote.example/users/alice/activities/announce/1")
        ));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(
            bindings[5],
            D1Type::Text("https://remote.example/users/bob/statuses/9")
        ));
        assert!(matches!(
            bindings[6],
            D1Type::Text("https://local.example/users/alice/statuses/2")
        ));
        assert!(matches!(bindings[7], D1Type::Text("")));
        assert!(matches!(bindings[8], D1Type::Text("")));
        assert!(matches!(bindings[9], D1Type::Text("unlisted")));
        assert!(matches!(bindings[10], D1Type::Integer(0)));
        assert!(matches!(bindings[11], D1Type::Null));
        assert!(matches!(bindings[12], D1Type::Text("accepted")));
        assert!(matches!(bindings[13], D1Type::Text("2026-05-11T01:02:03Z")));
        assert!(matches!(
            bindings[14],
            D1Type::Text("{\"type\":\"Announce\"}")
        ));
    }

    #[test]
    fn remote_status_content_html_prefers_content() {
        let object = json!({
            "content": "<p>Remote content</p>",
            "name": "Fallback name",
        });

        assert_eq!(remote_status_content_html(&object), "<p>Remote content</p>");
    }

    #[test]
    fn remote_status_content_html_renders_name_fallback() {
        let object = json!({
            "name": "Fallback name",
        });

        assert_eq!(
            remote_status_content_html(&object),
            render_status_html("Fallback name")
        );
    }

    #[test]
    fn remote_status_published_at_prefers_published() {
        let object = json!({
            "published": "2026-05-10T01:02:03Z",
            "updated": "2026-05-10T04:05:06Z",
        });

        assert_eq!(remote_status_published_at(&object), "2026-05-10T01:02:03Z");
    }

    #[test]
    fn remote_status_published_at_falls_back_to_updated() {
        let object = json!({
            "updated": "2026-05-10T04:05:06Z",
        });

        assert_eq!(remote_status_published_at(&object), "2026-05-10T04:05:06Z");
    }

    #[test]
    fn remote_status_language_extracts_content_map_key() {
        let object = json!({
            "contentMap": {
                "ja": "こんにちは",
            },
        });

        assert_eq!(remote_status_language(&object).as_deref(), Some("ja"));
    }

    #[test]
    fn remote_status_language_ignores_missing_content_map() {
        let object = json!({});

        assert_eq!(remote_status_language(&object), None);
    }
}
