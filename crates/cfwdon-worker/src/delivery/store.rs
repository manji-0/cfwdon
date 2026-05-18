use std::collections::{HashMap, HashSet};

use super::{D1Database, FollowerTargetRow};
use crate::{sql_placeholders, unique_ordered_refs};
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

#[derive(Debug, Deserialize)]
struct FollowerAccountTargetRow {
    account_id: String,
    target_inbox: String,
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

pub(crate) async fn list_follower_delivery_targets_by_account_ids(
    db: &D1Database,
    account_ids: &[String],
) -> Result<HashMap<String, Vec<String>>> {
    let ids = unique_ordered_refs(account_ids);
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = sql_placeholders(1, ids.len());
    let sql = format!(
        "SELECT account_id,
                COALESCE(NULLIF(shared_inbox_uri, ''), inbox_uri) AS target_inbox
         FROM followers
         WHERE account_id IN ({placeholders})
         GROUP BY account_id, COALESCE(NULLIF(shared_inbox_uri, ''), inbox_uri)
         ORDER BY account_id ASC, target_inbox ASC"
    );
    let bindings = ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    let mut by_account = HashMap::<String, Vec<String>>::new();
    for row in result.results::<FollowerAccountTargetRow>()? {
        if !row.target_inbox.trim().is_empty() {
            by_account
                .entry(row.account_id)
                .or_default()
                .push(row.target_inbox);
        }
    }
    Ok(by_account)
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

pub(crate) async fn expand_outbox_delivery_targets_for_deliveries(
    db: &D1Database,
    deliveries: &[&OutboxDeliveryRow],
    targets_by_account: &HashMap<String, Vec<String>>,
) -> Result<usize> {
    let mut expansions = Vec::new();
    for delivery in deliveries {
        let Some(targets) = targets_by_account.get(&delivery.account_id) else {
            continue;
        };
        let mut seen = HashSet::new();
        for target in targets {
            if seen.insert(target.as_str()) {
                expansions.push((*delivery, target.as_str()));
            }
        }
    }

    for chunk in expansions.chunks(OUTBOX_DELIVERY_EXPAND_CHUNK_SIZE) {
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
        for (delivery, target) in chunk {
            bindings.extend([
                D1Type::Text(delivery.account_id.as_str()),
                D1Type::Text(delivery.status_id.as_str()),
                D1Type::Text(delivery.activity_id.as_str()),
                D1Type::Text(delivery.activity_type.as_str()),
                D1Type::Text(target),
                D1Type::Text(delivery.payload_json.as_str()),
            ]);
        }
        db.prepare(&sql).bind_refs(bindings.iter())?.run().await?;
    }

    Ok(expansions.len())
}
