use serde::Deserialize;
use worker::d1::D1Type;

use super::{D1Database, Result, delivery_retry_delay_modifier};

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

pub(crate) async fn list_pending_outbound_activities(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<OutboundActivityRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, activity_id, activity_type, target_actor_uri, target_inbox, payload_json, attempt_count
             FROM outbound_activities
             WHERE state = 'queued'
               AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
             ORDER BY created_at ASC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<OutboundActivityRow>()
}

pub(crate) async fn mark_outbound_activity_delivered(
    db: &D1Database,
    activity_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text("delivered"), D1Type::Text(activity_id)];
    db.prepare(
        "UPDATE outbound_activities
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

pub(crate) async fn mark_outbound_activity_terminal_failure(
    db: &D1Database,
    activity_id: &str,
    next_attempt: u32,
) -> Result<()> {
    let bindings = [
        D1Type::Text("failed"),
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(activity_id),
    ];
    db.prepare(
        "UPDATE outbound_activities
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

pub(crate) fn outbound_terminal_failure_follow_state(activity_type: &str) -> Option<&'static str> {
    match activity_type {
        "Follow" => Some("failed"),
        _ => None,
    }
}

pub(crate) async fn reconcile_outbound_activity_terminal_failure(
    db: &D1Database,
    delivery: &OutboundActivityRow,
    next_attempt: u32,
) -> Result<()> {
    mark_outbound_activity_terminal_failure(db, &delivery.id, next_attempt).await?;

    if let Some(state) = outbound_terminal_failure_follow_state(&delivery.activity_type)
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
) -> Result<()> {
    let delay = delivery_retry_delay_modifier(next_attempt);
    let bindings = [
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delay),
        D1Type::Text(activity_id),
    ];
    db.prepare(
        "UPDATE outbound_activities
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
