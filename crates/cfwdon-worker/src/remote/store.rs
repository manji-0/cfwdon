use crate::{
    AppConfig, RemoteActorProfile, RemoteStatusAttachmentRow, build_remote_status_response,
    count_followers_by_actor, delete_remote_status_poll_by_status_id, extract_remote_poll_draft,
    find_account_by_id, find_local_status_by_object_uri, find_remote_actor_by_actor_uri,
    generate_entity_id, insert_remote_status_edit_snapshot, normalize_status_history_entry,
    now_iso_string, quote_target_uri_from_object,
    remote::adapters::{
        activity_pub_reblog_input_from_activity, activity_pub_status_input_from_object,
    },
    replace_remote_status_attachments, replace_remote_status_hashtags,
    send_remote_status_quote_notification, send_remote_status_update_notifications,
    upsert_remote_status_poll,
};
use cfwdon_domain::{
    QuoteState, RemoteQuoteLocalTarget, RemoteQuoteResolution, RemoteStatus, StatusId,
    StoredRemoteReblogIntent, StoredRemoteStatusIntent, merged_quote_state_for_remote_upsert,
    remote_status_default_quote_state,
};
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

pub(crate) type RemoteStatusRow = RemoteStatus;

pub(crate) use cfwdon_domain::RemoteStatusRecord;

pub(crate) fn remote_status_from_record(record: RemoteStatusRecord) -> RemoteStatusRow {
    RemoteStatus::from_record(record)
}

pub(crate) fn default_remote_quote_state() -> String {
    remote_status_default_quote_state()
}

pub(crate) fn effective_remote_status_quote_state(status: &RemoteStatusRow) -> &'static str {
    status.effective_quote_state().as_str()
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
    .first::<RemoteStatusRecord>(None)
    .await
    .map(|row| row.map(remote_status_from_record))
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
    .first::<RemoteStatusRecord>(None)
    .await
    .map(|row| row.map(remote_status_from_record))
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
    .first::<RemoteStatusRecord>(None)
    .await
    .map(|row| row.map(remote_status_from_record))
}

#[derive(Debug, Deserialize)]
struct RemoteStatusEditStateRow {
    #[serde(flatten)]
    record: RemoteStatusRecord,
    raw_object_json: String,
}

impl RemoteStatusEditStateRow {
    fn status_row(&self) -> RemoteStatusRow {
        remote_status_from_record(self.record.clone())
    }
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

fn build_remote_status_store_intent(
    actor: &RemoteActorProfile,
    object: &serde_json::Value,
    status_id: StatusId,
    quote_resolution: RemoteQuoteResolution,
    revision_at: String,
) -> Result<StoredRemoteStatusIntent> {
    let input = activity_pub_status_input_from_object(object);
    let incoming = input
        .into_incoming()
        .map_err(|error| Error::RustError(error.to_string()))?;
    Ok(incoming
        .into_store_intent(
            status_id,
            actor.actor_uri.clone(),
            quote_resolution,
            serialize_remote_status_object_json(object)?,
            revision_at,
        )
        .state)
}

fn build_remote_reblog_store_intent(
    actor: &RemoteActorProfile,
    activity: &serde_json::Value,
    status_id: StatusId,
    quote_resolution: RemoteQuoteResolution,
) -> Result<StoredRemoteReblogIntent> {
    let input = activity_pub_reblog_input_from_activity(activity);
    let incoming = input
        .into_incoming()
        .map_err(|error| Error::RustError(error.to_string()))?;
    let mut intent = incoming
        .into_store_intent(
            status_id,
            actor.actor_uri.clone(),
            quote_resolution,
            serialize_remote_reblog_activity_json(activity)?,
        )
        .state;
    if intent.published_at.is_empty() {
        intent.published_at = now_iso_string()?;
    }
    Ok(intent)
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
    let Some(actor) = find_remote_actor_by_actor_uri(db, &previous.record.actor_uri).await? else {
        return Ok(());
    };
    let response =
        build_remote_status_response(db, config, None, &previous.status_row(), &actor).await?;
    let mut snapshot = serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({}));
    snapshot["created_at"] = serde_json::json!(revision_at);
    let snapshot = normalize_status_history_entry(snapshot);
    let snapshot_json = serialize_remote_status_snapshot_json(&snapshot)?;
    insert_remote_status_edit_snapshot(db, &previous.record.id, &snapshot_json, revision_at).await
}

async fn upsert_remote_status_draft(
    db: &D1Database,
    intent: &StoredRemoteStatusIntent,
) -> Result<()> {
    let bindings = remote_status_upsert_bindings(intent);
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
        quote_state = excluded.quote_state,
        published_at = excluded.published_at,
        raw_object_json = excluded.raw_object_json,
        updated_at = ?16"
}

fn remote_status_upsert_bindings(intent: &StoredRemoteStatusIntent) -> [D1Type<'_>; 16] {
    [
        D1Type::Text(intent.status_id.as_str()),
        D1Type::Text(intent.actor_uri.as_str()),
        D1Type::Text(intent.object_uri.as_str()),
        optional_text_binding(intent.url.as_deref()),
        optional_text_binding(intent.in_reply_to_uri.as_deref()),
        D1Type::Null,
        optional_text_binding(intent.quote_of_uri.as_deref()),
        D1Type::Text(intent.content_html.as_str()),
        D1Type::Text(intent.spoiler_text.as_str()),
        D1Type::Text(intent.visibility.as_str()),
        bool_binding(intent.sensitive),
        optional_text_binding(intent.language.as_deref()),
        D1Type::Text(intent.quote_state.as_str()),
        D1Type::Text(intent.published_at.as_str()),
        D1Type::Text(intent.raw_object_json.as_str()),
        D1Type::Text(intent.revision_at.as_str()),
    ]
}

async fn insert_previous_remote_status_snapshot_if_changed(
    db: &D1Database,
    config: &AppConfig,
    previous: Option<&RemoteStatusEditStateRow>,
    intent: &StoredRemoteStatusIntent,
) -> Result<()> {
    if let Some(previous) =
        previous.filter(|existing| existing.raw_object_json != intent.raw_object_json)
    {
        insert_previous_remote_status_snapshot(db, config, previous, &intent.revision_at).await?;
    }

    Ok(())
}

async fn reload_upserted_remote_status(
    db: &D1Database,
    intent: &StoredRemoteStatusIntent,
) -> Result<RemoteStatusRow> {
    find_remote_status_by_object_uri(db, &intent.object_uri)
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
    intent: &StoredRemoteStatusIntent,
) {
    if previous_raw_object_json.is_none() {
        let _ = send_remote_status_quote_notification(
            db,
            config,
            &status.id,
            &status.actor_uri,
            status.quote_state.as_str(),
            status.quote_of_uri.as_deref(),
        )
        .await;
    } else if previous_raw_object_json != Some(intent.raw_object_json.as_str()) {
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
    let object_uri = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote status object is missing id".to_owned()))?;
    let previous = find_remote_status_edit_state_by_object_uri(db, object_uri).await?;
    let quote_of_uri = quote_target_uri_from_object(object);
    let quote_resolution =
        resolve_remote_quote_resolution(db, config, actor, quote_of_uri.as_deref()).await?;
    let revision_at = now_iso_string()?;
    let status_id = StatusId::new(generate_entity_id(16)?)
        .map_err(|error| Error::RustError(error.to_string()))?;
    let mut intent =
        build_remote_status_store_intent(actor, object, status_id, quote_resolution, revision_at)?;
    if let Some(ref previous) = previous {
        intent.quote_state = merged_quote_state_for_remote_upsert(
            previous.status_row().quote_state,
            intent.quote_state,
        );
    }

    insert_previous_remote_status_snapshot_if_changed(db, config, previous.as_ref(), &intent)
        .await?;
    upsert_remote_status_draft(db, &intent).await?;

    let status = reload_upserted_remote_status(db, &intent).await?;
    replace_remote_status_dependents(db, &status, object).await?;
    send_remote_status_change_notifications(
        db,
        config,
        previous
            .as_ref()
            .map(|value| value.raw_object_json.as_str()),
        &status,
        &intent,
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
    let object_uri = activity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote announce activity is missing id".to_owned()))?;
    let quote_of_uri = quote_target_uri_from_object(activity);
    let quote_resolution =
        resolve_remote_quote_resolution(db, config, remote_actor, quote_of_uri.as_deref()).await?;
    let status_id = StatusId::new(generate_entity_id(16)?)
        .map_err(|error| Error::RustError(error.to_string()))?;
    let mut intent =
        build_remote_reblog_store_intent(remote_actor, activity, status_id, quote_resolution)?;
    if let Some(previous) = find_remote_status_by_object_uri(db, object_uri).await? {
        intent.quote_state =
            merged_quote_state_for_remote_upsert(previous.quote_state, intent.quote_state);
    }

    upsert_remote_reblog_status_draft(db, &intent).await
}

async fn upsert_remote_reblog_status_draft(
    db: &D1Database,
    intent: &StoredRemoteReblogIntent,
) -> Result<()> {
    let bindings = remote_reblog_upsert_bindings(intent);
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
        quote_state = excluded.quote_state,
        published_at = excluded.published_at,
        raw_object_json = excluded.raw_object_json,
        updated_at = CURRENT_TIMESTAMP"
}

fn remote_reblog_upsert_bindings(intent: &StoredRemoteReblogIntent) -> [D1Type<'_>; 15] {
    [
        D1Type::Text(intent.status_id.as_str()),
        D1Type::Text(intent.actor_uri.as_str()),
        D1Type::Text(intent.object_uri.as_str()),
        D1Type::Null,
        D1Type::Null,
        D1Type::Text(intent.boost_of_uri.as_str()),
        optional_text_binding(intent.quote_of_uri.as_deref()),
        D1Type::Text(""),
        D1Type::Text(""),
        D1Type::Text(intent.visibility.as_str()),
        D1Type::Integer(0),
        D1Type::Null,
        D1Type::Text(intent.quote_state.as_str()),
        D1Type::Text(intent.published_at.as_str()),
        D1Type::Text(intent.raw_object_json.as_str()),
    ]
}

async fn resolve_remote_quote_resolution(
    db: &D1Database,
    config: &AppConfig,
    actor: &RemoteActorProfile,
    quote_of_uri: Option<&str>,
) -> Result<RemoteQuoteResolution> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok(RemoteQuoteResolution::without_quote());
    };
    let Some(status) = find_local_status_by_object_uri(db, config, quote_of_uri).await? else {
        return Ok(RemoteQuoteResolution::accepted_quote(
            quote_of_uri.to_owned(),
        ));
    };
    let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(RemoteQuoteResolution::accepted_quote(
            quote_of_uri.to_owned(),
        ));
    };
    let remote_actor_follows_owner =
        count_followers_by_actor(db, owner.id(), &actor.actor_uri).await? > 0;
    let blocked_by_owner = crate::is_blocking_actor(db, owner.id(), &actor.actor_uri).await?;
    let policy = status.effective_quote_approval_policy();
    Ok(RemoteQuoteResolution::with_local_target(
        quote_of_uri.to_owned(),
        RemoteQuoteLocalTarget {
            blocked_by_owner,
            policy_allows: policy.allows_quote(false, remote_actor_follows_owner),
        },
    ))
}

pub(crate) async fn clear_remote_status_quote(
    db: &D1Database,
    status: &RemoteStatusRow,
) -> Result<RemoteStatusRow> {
    update_remote_status_quote_state(
        db,
        &status.id,
        QuoteState::quote_state_after_revoke(status.quote_state),
    )
    .await
}

pub(crate) async fn update_remote_status_quote_state(
    db: &D1Database,
    status_id: &str,
    quote_state: QuoteState,
) -> Result<RemoteStatusRow> {
    let bindings = remote_status_quote_state_update_bindings(quote_state.as_str(), status_id);
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
    fn incoming_remote_status_requires_object_id() {
        let error = activity_pub_status_input_from_object(&json!({}))
            .into_incoming()
            .unwrap_err();

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
    fn remote_status_upsert_sql_writes_merged_quote_state() {
        let sql = remote_status_upsert_sql();

        assert!(sql.contains("ON CONFLICT(object_uri) DO UPDATE SET"));
        assert!(sql.contains("quote_state = excluded.quote_state"));
        assert!(sql.contains("updated_at = ?16"));
    }

    #[test]
    fn remote_reblog_upsert_sql_writes_merged_quote_state() {
        let sql = remote_reblog_upsert_sql();

        assert!(sql.contains("ON CONFLICT(object_uri) DO UPDATE SET"));
        assert!(sql.contains("quote_state = excluded.quote_state"));
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
    fn build_remote_status_store_intent_extracts_storage_fields() {
        use cfwdon_domain::{QuoteState, RemoteQuoteResolution, StatusId, Visibility};

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

        let intent = build_remote_status_store_intent(
            &actor,
            &object,
            StatusId::new("remote-status-id").expect("status id"),
            RemoteQuoteResolution::without_quote(),
            "revision-time".to_owned(),
        )
        .expect("store intent");

        assert_eq!(intent.actor_uri, actor.actor_uri);
        assert_eq!(
            intent.object_uri,
            "https://remote.example/users/alice/statuses/1"
        );
        assert_eq!(
            intent.url.as_deref(),
            Some("https://remote.example/@alice/1")
        );
        assert_eq!(
            intent.in_reply_to_uri.as_deref(),
            Some("https://remote.example/users/bob/statuses/9")
        );
        assert_eq!(intent.content_html, "<p>Hello</p>");
        assert_eq!(intent.spoiler_text, "spoiler");
        assert_eq!(intent.visibility, Visibility::Public);
        assert!(intent.sensitive);
        assert_eq!(intent.language.as_deref(), Some("ja"));
        assert_eq!(intent.quote_state, QuoteState::Accepted);
        assert_eq!(intent.published_at, "2026-05-10T01:02:03Z");
        assert_eq!(intent.revision_at, "revision-time");
        assert_eq!(intent.status_id.as_str(), "remote-status-id");
    }

    #[test]
    fn remote_status_upsert_bindings_keep_sql_slot_order_stable() {
        use cfwdon_domain::{QuoteState, StatusId, StoredRemoteStatusIntent, Visibility};

        let intent = StoredRemoteStatusIntent {
            status_id: StatusId::new("remote-status-id").expect("status id"),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: "https://remote.example/users/alice/statuses/1".to_owned(),
            url: Some("https://remote.example/@alice/1".to_owned()),
            in_reply_to_uri: Some("https://remote.example/users/bob/statuses/9".to_owned()),
            quote_of_uri: Some("https://local.example/users/alice/statuses/2".to_owned()),
            content_html: "<p>Hello</p>".to_owned(),
            spoiler_text: "spoiler".to_owned(),
            visibility: Visibility::Public,
            sensitive: true,
            language: Some("ja".to_owned()),
            quote_state: QuoteState::Accepted,
            published_at: "2026-05-10T01:02:03Z".to_owned(),
            raw_object_json: "{\"type\":\"Note\"}".to_owned(),
            revision_at: "revision-time".to_owned(),
        };
        let bindings = remote_status_upsert_bindings(&intent);

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
    fn build_remote_reblog_store_intent_extracts_storage_fields() {
        use cfwdon_domain::{QuoteState, RemoteQuoteResolution, StatusId, Visibility};

        let actor = remote_actor_profile_fixture();
        let activity = json!({
            "id": "https://remote.example/users/alice/activities/announce/1",
            "type": "Announce",
            "object": {
                "id": "https://remote.example/users/bob/statuses/9"
            },
            "quoteUri": "https://local.example/users/alice/statuses/2",
            "published": "2026-05-11T01:02:03Z",
            "to": ["https://www.w3.org/ns/activitystreams#Public"]
        });

        let intent = build_remote_reblog_store_intent(
            &actor,
            &activity,
            StatusId::new("remote-reblog-id").expect("status id"),
            RemoteQuoteResolution::accepted_quote(
                "https://local.example/users/alice/statuses/2".to_owned(),
            ),
        )
        .expect("store intent");

        assert_eq!(intent.status_id.as_str(), "remote-reblog-id");
        assert_eq!(intent.actor_uri, actor.actor_uri);
        assert_eq!(
            intent.object_uri,
            "https://remote.example/users/alice/activities/announce/1"
        );
        assert_eq!(
            intent.boost_of_uri,
            "https://remote.example/users/bob/statuses/9"
        );
        assert_eq!(
            intent.quote_of_uri.as_deref(),
            Some("https://local.example/users/alice/statuses/2")
        );
        assert_eq!(intent.visibility, Visibility::Public);
        assert_eq!(intent.quote_state, QuoteState::Accepted);
        assert_eq!(intent.published_at, "2026-05-11T01:02:03Z");
        assert!(intent.raw_object_json.contains("\"Announce\""));
    }

    #[test]
    fn build_remote_reblog_store_intent_fills_missing_published_at() {
        use cfwdon_domain::{RemoteQuoteResolution, StatusId};

        let actor = remote_actor_profile_fixture();
        let activity = json!({
            "id": "https://remote.example/users/alice/activities/announce/1",
            "type": "Announce",
            "object": "https://remote.example/users/bob/statuses/9"
        });

        let intent = build_remote_reblog_store_intent(
            &actor,
            &activity,
            StatusId::new("remote-reblog-id").expect("status id"),
            RemoteQuoteResolution::without_quote(),
        )
        .expect("store intent");

        assert!(!intent.published_at.is_empty());
    }

    #[test]
    fn remote_reblog_upsert_bindings_keep_sql_slot_order_stable() {
        use cfwdon_domain::{QuoteState, StatusId, StoredRemoteReblogIntent, Visibility};

        let intent = StoredRemoteReblogIntent {
            status_id: StatusId::new("remote-reblog-id").expect("status id"),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: "https://remote.example/users/alice/activities/announce/1".to_owned(),
            boost_of_uri: "https://remote.example/users/bob/statuses/9".to_owned(),
            quote_of_uri: Some("https://local.example/users/alice/statuses/2".to_owned()),
            visibility: Visibility::Unlisted,
            quote_state: QuoteState::Accepted,
            published_at: "2026-05-11T01:02:03Z".to_owned(),
            raw_object_json: "{\"type\":\"Announce\"}".to_owned(),
        };
        let bindings = remote_reblog_upsert_bindings(&intent);

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
    fn activity_pub_status_input_prefers_content_html() {
        let input = activity_pub_status_input_from_object(&json!({
            "content": "<p>Remote content</p>",
            "name": "Fallback name",
        }));

        assert_eq!(input.content_html, "<p>Remote content</p>");
    }

    #[test]
    fn incoming_remote_status_maps_published_at_and_language() {
        let incoming = activity_pub_status_input_from_object(&json!({
            "id": "https://remote.example/statuses/1",
            "published": "2026-05-10T01:02:03Z",
            "updated": "2026-05-10T04:05:06Z",
            "contentMap": {
                "ja": "こんにちは",
            },
        }))
        .into_incoming()
        .expect("incoming status");

        assert_eq!(incoming.published_at(), "2026-05-10T01:02:03Z");
        assert_eq!(incoming.language().as_deref(), Some("ja"));
    }

    #[test]
    fn incoming_remote_status_falls_back_to_updated_timestamp() {
        let incoming = activity_pub_status_input_from_object(&json!({
            "id": "https://remote.example/statuses/1",
            "updated": "2026-05-10T04:05:06Z",
        }))
        .into_incoming()
        .expect("incoming status");

        assert_eq!(incoming.published_at(), "2026-05-10T04:05:06Z");
    }
}
