use super::{
    AppConfig, D1Database, Error, LocalAccount, Result, StatusRow, actor_url,
    build_activitypub_delete, build_activitypub_note, describe_outbound_activity,
    is_public_activitypub_visibility, status_has_active_quote,
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

pub(crate) async fn enqueue_outbox_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Ok(());
    }

    let note = build_activitypub_note(db, config, account, status, false).await?;
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

    let bindings = [
        D1Type::Text(account.id()),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(activity_id.as_str()),
        D1Type::Text(payload_json.as_str()),
    ];
    db.prepare(
        "INSERT INTO outbox_deliveries (
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
            'Create',
            NULL,
            ?4,
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

    Ok(())
}

pub(crate) async fn enqueue_outbox_delete(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Ok(());
    }

    let activity = build_activitypub_delete(config, account, status)?;
    let activity_id = activity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("activitypub delete id missing".to_owned()))?;
    let payload_json = serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!("failed to serialize delete activity: {error}"))
    })?;

    let bindings = [
        D1Type::Text(account.id()),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(activity_id),
        D1Type::Text(payload_json.as_str()),
    ];
    db.prepare(
        "INSERT INTO outbox_deliveries (
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
            'Delete',
            NULL,
            ?4,
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

    Ok(())
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
