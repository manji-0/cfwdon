use std::collections::HashSet;

use super::{D1Database, FollowerTargetRow};
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

#[derive(Debug, Deserialize)]
pub(crate) struct OutboxDeliveryRow {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) status_id: String,
    pub(crate) activity_id: String,
    pub(crate) activity_type: String,
    pub(crate) target_inbox: Option<String>,
    pub(crate) payload_json: String,
    pub(crate) attempt_count: i32,
}

pub(crate) async fn list_pending_generic_outbox_deliveries(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<OutboxDeliveryRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, status_id, activity_id, activity_type, target_inbox, payload_json, attempt_count
             FROM outbox_deliveries
             WHERE state = 'queued'
               AND target_inbox IS NULL
               AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
             ORDER BY created_at ASC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<OutboxDeliveryRow>()
}

pub(crate) async fn list_pending_target_outbox_deliveries(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<OutboxDeliveryRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, status_id, activity_id, activity_type, target_inbox, payload_json, attempt_count
             FROM outbox_deliveries
             WHERE state = 'queued'
               AND target_inbox IS NOT NULL
               AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
             ORDER BY created_at ASC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<OutboxDeliveryRow>()
}

pub(crate) async fn list_follower_delivery_targets(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT DISTINCT COALESCE(NULLIF(shared_inbox_uri, ''), inbox_uri) AS target_inbox
             FROM followers
             WHERE account_id = ?1
             ORDER BY target_inbox ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<FollowerTargetRow>()?
        .into_iter()
        .map(|row| row.target_inbox)
        .filter(|value| !value.trim().is_empty())
        .collect())
}

pub(crate) async fn list_follower_actor_uris(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT actor_uri AS target_inbox
             FROM followers
             WHERE account_id = ?1
             ORDER BY created_at ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<FollowerTargetRow>()?
        .into_iter()
        .map(|row| row.target_inbox)
        .filter(|value| !value.trim().is_empty())
        .collect())
}

pub(crate) async fn expand_outbox_delivery_targets(
    db: &D1Database,
    delivery: &OutboxDeliveryRow,
    targets: &[String],
) -> Result<usize> {
    let mut seen = HashSet::new();
    let mut inserted = 0usize;

    for target in targets {
        if !seen.insert(target.clone()) {
            continue;
        }

        let bindings = [
            D1Type::Text(delivery.account_id.as_str()),
            D1Type::Text(delivery.status_id.as_str()),
            D1Type::Text(delivery.activity_id.as_str()),
            D1Type::Text(delivery.activity_type.as_str()),
            D1Type::Text(target.as_str()),
            D1Type::Text(delivery.payload_json.as_str()),
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
        inserted += 1;
    }

    Ok(inserted)
}
