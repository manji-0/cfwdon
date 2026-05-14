use super::{D1Database, ExpiredPollQueueRow, Result};
use worker::d1::D1Type;

pub(crate) async fn list_expired_polls_requiring_federation_close(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<ExpiredPollQueueRow>> {
    let bindings = [D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT p.id AS poll_id,
                    p.status_id,
                    s.account_id
             FROM status_polls p
             JOIN statuses s
               ON s.id = p.status_id
             WHERE p.federated_closed_at IS NULL
               AND s.visibility IN ('public', 'unlisted')
               AND datetime(replace(replace(p.expires_at, 'T', ' '), 'Z', '')) <= CURRENT_TIMESTAMP
             ORDER BY p.expires_at ASC
             LIMIT ?1",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<ExpiredPollQueueRow>()
}

pub(crate) async fn mark_status_poll_federated_closed(
    db: &D1Database,
    poll_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(poll_id)];
    db.prepare(
        "UPDATE status_polls
         SET federated_closed_at = CURRENT_TIMESTAMP,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}
