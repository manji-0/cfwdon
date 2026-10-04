use crate::tracked_d1::D1Database;
use serde::Deserialize;
use worker::{Result, d1::D1Type};

#[derive(Debug, Deserialize)]
struct IdempotentStatusRow {
    status_id: String,
}

/// Status created by `account_id` with this Idempotency-Key within the last hour.
pub(crate) async fn find_idempotent_status_id(
    db: &D1Database,
    account_id: &str,
    idempotency_key: &str,
) -> Result<Option<String>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(idempotency_key)];
    Ok(db
        .prepare(
            "SELECT status_id
             FROM status_idempotency_keys
             WHERE account_id = ?1
               AND idempotency_key = ?2
               AND created_at > datetime('now', '-1 hour')
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<IdempotentStatusRow>(None)
        .await?
        .map(|row| row.status_id))
}

pub(crate) async fn record_idempotent_status(
    db: &D1Database,
    account_id: &str,
    idempotency_key: &str,
    status_id: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(idempotency_key),
        D1Type::Text(status_id),
    ];
    db.prepare(
        "INSERT OR REPLACE INTO status_idempotency_keys (account_id, idempotency_key, status_id)
         VALUES (?1, ?2, ?3)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn purge_expired_status_idempotency_keys(db: &D1Database) -> Result<()> {
    db.prepare(
        "DELETE FROM status_idempotency_keys
         WHERE created_at <= datetime('now', '-1 hour')",
    )
    .run()
    .await?;
    Ok(())
}
