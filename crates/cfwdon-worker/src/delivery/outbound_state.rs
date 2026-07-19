use serde::Deserialize;
use worker::d1::D1Type;

use cfwdon_domain::{
    RemoteFollowState, delivery_retry_delay_modifier, outbound_terminal_failure_follow_state,
};

use super::{D1Database, OUTBOX_IN_FLIGHT_STALE_MODIFIER, Result};

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

#[derive(Debug, Deserialize)]
pub(crate) struct OutboundActivityRow {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) activity_id: String,
    pub(crate) activity_type: String,
    pub(crate) target_actor_uri: Option<String>,
    pub(crate) target_inbox: String,
    pub(crate) payload_json: String,
    pub(crate) attempt_count: i32,
}

pub(crate) async fn claim_pending_outbound_activities(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<OutboundActivityRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "UPDATE outbound_activities
             SET state = 'in_flight',
                 last_attempt_at = CURRENT_TIMESTAMP,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id IN (
                 SELECT id
                 FROM outbound_activities
                 WHERE state = 'queued'
                   AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
                 ORDER BY created_at ASC
                 LIMIT ?1
             )
             RETURNING id, account_id, activity_id, activity_type, target_actor_uri, target_inbox, payload_json, attempt_count",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<OutboundActivityRow>()
}

pub(crate) async fn mark_outbound_activity_delivered(
    db: &D1Database,
    activity_id: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text("delivered"), D1Type::Text(activity_id)];
    let result = db
        .prepare(
            "UPDATE outbound_activities
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

pub(crate) async fn mark_outbound_activity_terminal_failure(
    db: &D1Database,
    activity_id: &str,
    next_attempt: u32,
) -> Result<bool> {
    let bindings = [
        D1Type::Text("failed"),
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(activity_id),
    ];
    let result = db
        .prepare(
            "UPDATE outbound_activities
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

pub(crate) fn outbound_terminal_failure_follow_state_name(
    activity_type: &str,
) -> Option<&'static str> {
    outbound_terminal_failure_follow_state(activity_type).map(RemoteFollowState::as_str)
}

pub(crate) async fn reconcile_outbound_activity_terminal_failure(
    db: &D1Database,
    delivery: &OutboundActivityRow,
    next_attempt: u32,
) -> Result<()> {
    mark_outbound_activity_terminal_failure(db, &delivery.id, next_attempt).await?;

    if let Some(state) = outbound_terminal_failure_follow_state_name(&delivery.activity_type)
        && let Some(target_actor_uri) = delivery.target_actor_uri.as_deref()
    {
        let bindings = [
            D1Type::Text(state),
            D1Type::Text(delivery.account_id.as_str()),
            D1Type::Text(target_actor_uri),
            D1Type::Text(delivery.activity_id.as_str()),
        ];
        db.prepare(
            "UPDATE follows
             SET state = ?1,
                 updated_at = CURRENT_TIMESTAMP
             WHERE follower_account_id = ?2
               AND target_actor_uri = ?3
               AND follow_activity_id = ?4
               AND state = 'pending'",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

pub(crate) async fn reschedule_outbound_activity(
    db: &D1Database,
    activity_id: &str,
    next_attempt: u32,
) -> Result<bool> {
    let delay = delivery_retry_delay_modifier(next_attempt);
    let bindings = [
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delay),
        D1Type::Text(activity_id),
    ];
    let result = db
        .prepare(
            "UPDATE outbound_activities
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

pub(crate) async fn requeue_stale_in_flight_outbound_activities(db: &D1Database) -> Result<()> {
    let stale_before = D1Type::Text(OUTBOX_IN_FLIGHT_STALE_MODIFIER);
    let max_attempts = D1Type::Integer(cfwdon_domain::DELIVERY_MAX_ATTEMPTS as i32);
    db.prepare(
        "UPDATE outbound_activities
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
