use cfwdon_domain::delivery_retry_delay_modifier;

use super::{D1Database, Result};
use crate::sql_placeholders;
use worker::d1::D1Type;

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
         WHERE id IN ({placeholders})"
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
) -> Result<()> {
    let bindings = [D1Type::Text("delivered"), D1Type::Text(delivery_id)];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn mark_outbox_delivery_terminal_failure(
    db: &D1Database,
    delivery_id: &str,
    next_attempt: u32,
) -> Result<()> {
    let bindings = [
        D1Type::Text("failed"),
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delivery_id),
    ];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = ?1,
             attempt_count = ?2,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn reschedule_outbox_delivery(
    db: &D1Database,
    delivery_id: &str,
    next_attempt: u32,
) -> Result<()> {
    let delay = delivery_retry_delay_modifier(next_attempt);
    let bindings = [
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delay),
        D1Type::Text(delivery_id),
    ];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = 'queued',
             attempt_count = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = datetime(CURRENT_TIMESTAMP, ?2),
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}
