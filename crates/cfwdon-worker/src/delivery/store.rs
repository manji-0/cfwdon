use std::collections::HashSet;

use super::{D1Database, FollowerTargetRow};
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

const OUTBOX_DELIVERY_EXPAND_CHUNK_SIZE: usize = 40;

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

#[derive(Debug, Deserialize)]
struct PendingOutboxWorkRow {
    has_pending: i32,
}

pub(crate) async fn pending_outbox_work_exists(db: &D1Database) -> Result<bool> {
    let row = db
        .prepare(
            "SELECT (
                EXISTS (
                    SELECT 1
                    FROM outbox_deliveries
                    WHERE state = 'queued'
                      AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
                )
                OR EXISTS (
                    SELECT 1
                    FROM outbound_activities
                    WHERE state = 'queued'
                      AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
                )
            ) AS has_pending",
        )
        .first::<PendingOutboxWorkRow>(None)
        .await?;

    Ok(row.is_some_and(|row| row.has_pending != 0))
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
    let unique_targets = targets
        .iter()
        .filter(|target| seen.insert((*target).clone()))
        .collect::<Vec<_>>();

    for chunk in unique_targets.chunks(OUTBOX_DELIVERY_EXPAND_CHUNK_SIZE) {
        let values = chunk
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let offset = index * 6;
                format!(
                    "(lower(hex(randomblob(16))), ?{}, ?{}, ?{}, ?{}, ?{}, ?{}, 'queued', 0, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
                    offset + 1,
                    offset + 2,
                    offset + 3,
                    offset + 4,
                    offset + 5,
                    offset + 6
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
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
            ) VALUES {values}"
        );
        let mut bindings = Vec::with_capacity(chunk.len() * 6);
        for target in chunk {
            bindings.extend([
                D1Type::Text(delivery.account_id.as_str()),
                D1Type::Text(delivery.status_id.as_str()),
                D1Type::Text(delivery.activity_id.as_str()),
                D1Type::Text(delivery.activity_type.as_str()),
                D1Type::Text(target.as_str()),
                D1Type::Text(delivery.payload_json.as_str()),
            ]);
        }
        db.prepare(&sql).bind_refs(bindings.iter())?.run().await?;
    }

    Ok(unique_targets.len())
}
