use super::{
    AppConfig, D1Database, Error, LocalAccount, Result, StatusRow, Visibility,
    activitypub_audiences_for_status, actor_url, build_activitypub_delete, build_activitypub_note,
    describe_outbound_activity, load_remote_actor_delivery_inbox, local_username_from_actor_uri,
    status_has_active_quote,
};
use std::collections::HashSet;
use worker::d1::D1Type;

fn create_activity_context(status: &StatusRow) -> serde_json::Value {
    if status_has_active_quote(status) {
        serde_json::json!([
            "https://www.w3.org/ns/activitystreams",
            {
                "_misskey_quote": {
                    "@id": "https://misskey-hub.net/ns#_misskey_quote",
                    "@type": "@id"
                }
            }
        ])
    } else {
        serde_json::json!("https://www.w3.org/ns/activitystreams")
    }
}

const OUTBOX_DELIVERY_INSERT_SQL: &str = "INSERT INTO outbox_deliveries (
    id,
    account_id,
    status_id,
    activity_id,
    activity_type,
    target_inbox,
    payload_json,
    state,
    attempt_count,
    last_attempt_at,
    next_attempt_at,
    created_at,
    updated_at
) VALUES (
    lower(hex(randomblob(16))),
    ?1,
    ?2,
    ?3,
    ?4,
    NULL,
    ?5,
    'queued',
    0,
    NULL,
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
)";

fn outbox_delivery_insert_statement(
    db: &D1Database,
    account_id: &str,
    status_id: &str,
    activity_id: &str,
    activity_type: &str,
    payload_json: &str,
) -> Result<worker::d1::D1PreparedStatement> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status_id),
        D1Type::Text(activity_id),
        D1Type::Text(activity_type),
        D1Type::Text(payload_json),
    ];
    db.prepare(OUTBOX_DELIVERY_INSERT_SQL)
        .bind_refs(bindings.iter())
}

fn audience_uri_strings(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(uri) => vec![uri.clone()],
        serde_json::Value::Array(values) => values.iter().flat_map(audience_uri_strings).collect(),
        serde_json::Value::Object(map) => map
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

async fn remote_direct_delivery_inboxes(
    db: &D1Database,
    config: &AppConfig,
    to_audiences: &serde_json::Value,
) -> Result<Vec<String>> {
    let mut inboxes = Vec::new();
    let mut seen = HashSet::new();

    for actor_uri in audience_uri_strings(to_audiences) {
        if local_username_from_actor_uri(config, &actor_uri).is_some() {
            continue;
        }
        if cfwdon_domain::is_followers_collection_uri(&actor_uri)
            || cfwdon_domain::is_public_audience_uri(&actor_uri)
        {
            continue;
        }
        let Some(inbox) = load_remote_actor_delivery_inbox(db, &actor_uri).await? else {
            continue;
        };
        if seen.insert(inbox.clone()) {
            inboxes.push(inbox);
        }
    }

    Ok(inboxes)
}

async fn build_create_activity_payload(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    attachments_override: Option<&[crate::MediaAttachmentRow]>,
) -> Result<(String, String)> {
    let note =
        build_activitypub_note(db, config, account, status, false, attachments_override).await?;
    let note_id = note
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("activitypub note id missing".to_owned()))?;
    let activity_id = format!("{note_id}/activity");
    let activity = serde_json::json!({
        "@context": create_activity_context(status),
        "type": "Create",
        "id": activity_id,
        "actor": actor_url(config, account.username()),
        "published": status.created_at.clone(),
        "to": note.get("to").cloned().unwrap_or_else(|| serde_json::json!([])),
        "cc": note.get("cc").cloned().unwrap_or_else(|| serde_json::json!([])),
        "object": note,
    });
    let payload_json = serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!("failed to serialize queued activity: {error}"))
    })?;
    Ok((activity_id, payload_json))
}

/// Generic follower-fanout Create (public / unlisted / followers-only).
/// Direct Creates are handled by [`enqueue_direct_create_activity`] after the
/// status row exists, because they need per-recipient inbox targeting.
pub(crate) async fn outbox_create_insert_statement(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<Option<worker::d1::D1PreparedStatement>> {
    outbox_create_insert_statement_with_attachments(db, config, account, status, None).await
}

pub(crate) async fn outbox_create_insert_statement_with_attachments(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    attachments_override: Option<&[crate::MediaAttachmentRow]>,
) -> Result<Option<worker::d1::D1PreparedStatement>> {
    if status.visibility == Visibility::Direct {
        return Ok(None);
    }

    let (activity_id, payload_json) =
        build_create_activity_payload(db, config, account, status, attachments_override).await?;

    outbox_delivery_insert_statement(
        db,
        account.id(),
        &status.id,
        &activity_id,
        "Create",
        &payload_json,
    )
    .map(Some)
}

pub(crate) async fn enqueue_direct_create_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    attachments_override: Option<&[crate::MediaAttachmentRow]>,
) -> Result<()> {
    if status.visibility != Visibility::Direct {
        return Ok(());
    }

    let (to_audiences, _) = activitypub_audiences_for_status(db, config, account, status).await?;
    let inboxes = remote_direct_delivery_inboxes(db, config, &to_audiences).await?;
    if inboxes.is_empty() {
        return Ok(());
    }

    let (_, payload_json) =
        build_create_activity_payload(db, config, account, status, attachments_override).await?;
    enqueue_targeted_outbox_activity(db, account.id(), &status.id, &payload_json, &inboxes).await
}

pub(crate) async fn outbox_delete_insert_statement(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<Option<worker::d1::D1PreparedStatement>> {
    if status.visibility == Visibility::Direct {
        return Ok(None);
    }

    let activity = build_activitypub_delete(db, config, account, status).await?;
    let activity_id = activity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("activitypub delete id missing".to_owned()))?;
    let payload_json = serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!("failed to serialize delete activity: {error}"))
    })?;

    outbox_delivery_insert_statement(
        db,
        account.id(),
        &status.id,
        activity_id,
        "Delete",
        &payload_json,
    )
    .map(Some)
}

pub(crate) async fn enqueue_direct_delete_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if status.visibility != Visibility::Direct {
        return Ok(());
    }

    let (to_audiences, _) = activitypub_audiences_for_status(db, config, account, status).await?;
    let inboxes = remote_direct_delivery_inboxes(db, config, &to_audiences).await?;
    if inboxes.is_empty() {
        return Ok(());
    }

    let activity = build_activitypub_delete(db, config, account, status).await?;
    let payload_json = serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!("failed to serialize delete activity: {error}"))
    })?;
    enqueue_targeted_outbox_activity(db, account.id(), &status.id, &payload_json, &inboxes).await
}

pub(crate) async fn enqueue_targeted_outbox_activity(
    db: &D1Database,
    account_id: &str,
    status_id: &str,
    payload_json: &str,
    target_inboxes: &[String],
) -> Result<()> {
    let descriptor = describe_outbound_activity(payload_json)?;
    let mut seen = HashSet::new();

    for target_inbox in target_inboxes {
        let target_inbox = target_inbox.trim();
        if target_inbox.is_empty() || !seen.insert(target_inbox.to_owned()) {
            continue;
        }

        let bindings = [
            D1Type::Text(account_id),
            D1Type::Text(status_id),
            D1Type::Text(descriptor.activity_id.as_str()),
            D1Type::Text(descriptor.activity_type.as_str()),
            D1Type::Text(target_inbox),
            D1Type::Text(payload_json),
        ];
        db.prepare(
            "INSERT OR IGNORE INTO outbox_deliveries (
                id,
                account_id,
                status_id,
                activity_id,
                activity_type,
                target_inbox,
                payload_json,
                state,
                attempt_count,
                last_attempt_at,
                next_attempt_at,
                created_at,
                updated_at
            ) VALUES (
                lower(hex(randomblob(16))),
                ?1,
                ?2,
                ?3,
                ?4,
                ?5,
                ?6,
                'queued',
                0,
                NULL,
                CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}
