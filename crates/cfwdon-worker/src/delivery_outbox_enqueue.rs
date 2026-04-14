use super::{
    AppConfig, D1Database, Error, LocalAccount, Result, StatusRow, actor_url,
    build_activitypub_delete, build_activitypub_note, is_public_activitypub_visibility,
};
use worker::d1::D1Type;

pub(crate) async fn enqueue_outbox_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if !is_public_activitypub_visibility(&status.visibility) {
        return Ok(());
    }

    let note = build_activitypub_note(db, config, account, status, false).await?;
    let note_id = note
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("activitypub note id missing".to_owned()))?;
    let activity_id = format!("{note_id}/activity");
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "type": "Create",
        "id": activity_id,
        "actor": actor_url(config, &account.username),
        "published": status.created_at.clone(),
        "to": note.get("to").cloned().unwrap_or_else(|| serde_json::json!([])),
        "cc": note.get("cc").cloned().unwrap_or_else(|| serde_json::json!([])),
        "object": note,
    });
    let payload_json = serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!("failed to serialize queued activity: {error}"))
    })?;

    let bindings = [
        D1Type::Text(account.id.as_str()),
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
    if !is_public_activitypub_visibility(&status.visibility) {
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
        D1Type::Text(account.id.as_str()),
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
