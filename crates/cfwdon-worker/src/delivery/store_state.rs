use cfwdon_domain::delivery_retry_delay_modifier;

use super::{D1Database, OUTBOX_IN_FLIGHT_STALE_MODIFIER, Result};
use crate::sql_placeholders;
use worker::d1::D1Type;

fn d1_result_did_change(result: &worker::d1::D1Result) -> Result<bool> {
    Ok(result
        .meta()?
        .and_then(|meta| {
            meta.changed_db
                .or_else(|| meta.changes.map(|changes| changes > 0))
                .or_else(|| meta.rows_written.map(|rows_written| rows_written > 0))
        })
        .unwrap_or(false))
}

pub(crate) async fn mark_outbox_deliveries_expanded(
    db: &D1Database,
    delivery_ids: &[String],
) -> Result<()> {
    if delivery_ids.is_empty() {
        return Ok(());
    }

    let placeholders = sql_placeholders(1, delivery_ids.len());
    let sql = format!(
        "UPDATE outbox_deliveries
         SET state = 'expanded',
             updated_at = CURRENT_TIMESTAMP
         WHERE id IN ({placeholders})
           AND state = 'in_flight'"
    );
    let bindings = delivery_ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    db.prepare(&sql).bind_refs(bindings.iter())?.run().await?;

    Ok(())
}

pub(crate) async fn mark_outbox_deliveries_completed_without_targets(
    db: &D1Database,
    delivery_ids: &[String],
) -> Result<()> {
    if delivery_ids.is_empty() {
        return Ok(());
    }

    let placeholders = sql_placeholders(1, delivery_ids.len());
    let sql = format!(
        "UPDATE outbox_deliveries
         SET state = 'delivered',
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id IN ({placeholders})"
    );
    let bindings = delivery_ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    db.prepare(&sql).bind_refs(bindings.iter())?.run().await?;

    Ok(())
}

pub(crate) async fn mark_outbox_delivery_delivered(
    db: &D1Database,
    delivery_id: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text("delivered"), D1Type::Text(delivery_id)];
    let result = db
        .prepare(
            "UPDATE outbox_deliveries
         SET state = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2
           AND state = 'in_flight'",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    d1_result_did_change(&result)
}

pub(crate) async fn mark_outbox_delivery_terminal_failure(
    db: &D1Database,
    delivery_id: &str,
    next_attempt: u32,
) -> Result<bool> {
    let bindings = [
        D1Type::Text("failed"),
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delivery_id),
    ];
    let result = db
        .prepare(
            "UPDATE outbox_deliveries
         SET state = ?1,
             attempt_count = ?2,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3
           AND state = 'in_flight'",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    d1_result_did_change(&result)
}

pub(crate) async fn reschedule_outbox_delivery(
    db: &D1Database,
    delivery_id: &str,
    next_attempt: u32,
) -> Result<bool> {
    let delay = delivery_retry_delay_modifier(next_attempt);
    let bindings = [
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delay),
        D1Type::Text(delivery_id),
    ];
    let result = db
        .prepare(
            "UPDATE outbox_deliveries
         SET state = 'queued',
             attempt_count = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = datetime(CURRENT_TIMESTAMP, ?2),
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3
           AND state = 'in_flight'",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    d1_result_did_change(&result)
}

pub(crate) async fn requeue_stale_in_flight_outbox_deliveries(db: &D1Database) -> Result<()> {
    let stale_before = D1Type::Text(OUTBOX_IN_FLIGHT_STALE_MODIFIER);
    let max_attempts = D1Type::Integer(cfwdon_domain::DELIVERY_MAX_ATTEMPTS as i32);
    db.prepare(
        "UPDATE outbox_deliveries
         SET attempt_count = attempt_count + 1,
             state = CASE
                 WHEN attempt_count + 1 >= ?2 THEN 'failed'
                 ELSE 'queued'
             END,
             next_attempt_at = CASE
                 WHEN attempt_count + 1 >= ?2 THEN next_attempt_at
                 ELSE CURRENT_TIMESTAMP
             END,
             updated_at = CURRENT_TIMESTAMP
         WHERE state = 'in_flight'
           AND last_attempt_at <= datetime(CURRENT_TIMESTAMP, ?1)",
    )
    .bind_refs(&[stale_before, max_attempts])?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn cancel_pending_outbox_deliveries_for_inbox(
    db: &D1Database,
    account_id: &str,
    target_inbox: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_inbox)];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = 'failed',
             updated_at = CURRENT_TIMESTAMP
         WHERE account_id = ?1
           AND target_inbox = ?2
           AND state IN ('queued', 'expanded', 'in_flight')",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}
