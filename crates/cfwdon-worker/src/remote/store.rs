mod attachments;
mod bindings;
mod edit_snapshots;
mod intents;
mod lookups;
mod notifications;
mod quotes;
mod records;
mod upsert_sql;

pub(crate) use attachments::*;
pub(crate) use lookups::*;
pub(crate) use notifications::*;
pub(crate) use quotes::*;
pub(crate) use records::*;

use crate::{
    AppConfig, D1Database, RemoteActorProfile, RemoteStatusAttachmentRow,
    build_remote_status_card_value, card_unfurl_payload, delete_remote_status_poll_by_status_id,
    extract_federated_emojis_from_activitypub_object, extract_remote_poll_draft,
    find_local_status_by_object_uri, generate_entity_id, now_iso_string,
    quote_target_uri_from_object, replace_remote_status_attachments,
    replace_remote_status_hashtags, replace_remote_status_mentions, resolve_in_reply_to_payload,
    soft_enqueue_background_job, upsert_remote_status_poll,
};
use cfwdon_domain::{
    StatusId, StoredRemoteReblogIntent, StoredRemoteStatusIntent,
    merged_quote_state_for_remote_upsert,
};
use worker::d1::D1Type;
use worker::{Env, Error, Result};

use bindings::optional_text_binding;
use edit_snapshots::{
    find_remote_status_edit_state_by_object_uri, insert_previous_remote_status_snapshot_if_changed,
};
use intents::{build_remote_reblog_store_intent, build_remote_status_store_intent};
use upsert_sql::{
    remote_reblog_upsert_bindings, remote_reblog_upsert_sql, remote_status_upsert_bindings,
    remote_status_upsert_sql,
};

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
    config: &AppConfig,
    status: &RemoteStatusRow,
    object: &serde_json::Value,
) -> Result<Vec<RemoteStatusAttachmentRow>> {
    replace_remote_status_hashtags(
        db,
        &status.id,
        &status.actor_uri,
        &status.published_at,
        &status.content_html,
    )
    .await?;
    let attachments = remote_status_attachments_from_object(&status.id, object);
    replace_remote_status_attachments(db, &status.id, &attachments).await?;
    if let Some(poll) = extract_remote_poll_draft(object) {
        upsert_remote_status_poll(db, &status.id, &poll).await?;
    } else {
        delete_remote_status_poll_by_status_id(db, &status.id).await?;
    }
    replace_remote_status_mentions(
        db,
        config,
        &status.id,
        &status.published_at,
        object,
        &status.plain_text(),
    )
    .await?;

    Ok(attachments)
}

async fn update_remote_status_card_json(
    db: &D1Database,
    status_id: &str,
    card_json: Option<String>,
) -> Result<()> {
    let card_value = optional_text_binding(card_json.as_deref());
    let id_value = D1Type::Text(status_id);
    db.prepare(
        "UPDATE remote_statuses
         SET card_json = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(&[card_value, id_value])?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn upsert_remote_status(
    db: &D1Database,
    config: &AppConfig,
    actor: &RemoteActorProfile,
    object: &serde_json::Value,
    env: Option<&Env>,
) -> Result<()> {
    let _env = env;
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
            previous.status_row()?.quote_state,
            intent.quote_state,
        );
        // Mark as edited when content has changed.
        if previous.raw_object_json != intent.raw_object_json {
            intent.edited_at = Some(intent.revision_at.clone());
        }
    }

    // Denormalize federated emoji map.
    intent.federated_emojis_json =
        serde_json::to_string(&extract_federated_emojis_from_activitypub_object(object))
            .unwrap_or_else(|_| "{}".to_owned());

    // Resolve in_reply_to_id from the URI if present.
    if let Some(ref uri) = intent.in_reply_to_uri.clone() {
        if let Some(remote) = find_remote_status_by_url_or_object_uri(db, uri).await? {
            intent.in_reply_to_id = Some(remote.id);
        } else if let Some(local) = find_local_status_by_object_uri(db, config, uri).await? {
            intent.in_reply_to_id = Some(local.id);
        }
    }

    insert_previous_remote_status_snapshot_if_changed(db, config, previous.as_ref(), &intent)
        .await?;
    upsert_remote_status_draft(db, &intent).await?;

    let status = reload_upserted_remote_status(db, &intent).await?;
    let attachments = replace_remote_status_dependents(db, config, &status, object).await?;

    // Compute card from plain text + attachments and persist it.
    let card_json = build_remote_status_card_value(&status.plain_text(), &attachments)
        .and_then(|v| serde_json::to_string(&v).ok());
    update_remote_status_card_json(db, &status.id, card_json).await?;

    // Soft-enqueue a card unfurl job for link preview enrichment.
    let _ = soft_enqueue_background_job(
        db,
        crate::JOB_CARD_UNFURL,
        &card_unfurl_payload("remote", &status.id),
        &intent.revision_at,
    )
    .await;

    // Soft-enqueue in_reply_to resolution if still unresolved.
    if status.in_reply_to_id.is_none() && status.in_reply_to_uri.is_some() {
        let _ = soft_enqueue_background_job(
            db,
            crate::JOB_RESOLVE_IN_REPLY_TO,
            &resolve_in_reply_to_payload(&status.id),
            &intent.revision_at,
        )
        .await;
    }

    let notification_kind = if previous.is_none() {
        Some("create")
    } else if previous
        .as_ref()
        .is_some_and(|value| value.raw_object_json != intent.raw_object_json)
    {
        Some("update")
    } else {
        None
    };
    if let Some(kind) = notification_kind {
        let _ = soft_enqueue_background_job(
            db,
            crate::JOB_REMOTE_STATUS_NOTIFY,
            &remote_status_notify_payload(&status.id, &actor.actor_uri, kind),
            &intent.revision_at,
        )
        .await;
    }

    Ok(())
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
