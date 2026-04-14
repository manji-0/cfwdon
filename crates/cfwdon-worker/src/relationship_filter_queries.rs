use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct MuteRow {
    pub(crate) notifications: i32,
    pub(crate) expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MuteEntryRow {
    pub(crate) cursor_id: i64,
    pub(crate) target_account_id: Option<String>,
    pub(crate) target_actor_uri: String,
}

pub(crate) async fn is_blocking_actor(
    db: &D1Database,
    blocker_account_id: &str,
    target_actor_uri: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(blocker_account_id),
        D1Type::Text(target_actor_uri),
    ];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM blocks
             WHERE blocker_account_id = ?1
               AND target_actor_uri = ?2",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

pub(crate) async fn find_active_mute(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
) -> Result<Option<MuteRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_actor_uri)];
    db.prepare(
        "DELETE FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri = ?2
           AND expires_at IS NOT NULL
           AND expires_at <= CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "SELECT notifications, expires_at
         FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<MuteRow>(None)
    .await
}

pub(crate) async fn is_muted_actor(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
) -> Result<bool> {
    Ok(find_active_mute(db, account_id, target_actor_uri)
        .await?
        .is_some())
}

pub(crate) async fn muted_notifications_for_actor(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
) -> Result<bool> {
    Ok(find_active_mute(db, account_id, target_actor_uri)
        .await?
        .map(|row| row.notifications != 0)
        .unwrap_or(false))
}

pub(crate) async fn list_mutes_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<MuteEntryRow>> {
    let account_id_binding = D1Type::Text(account_id);
    db.prepare(
        "DELETE FROM mutes
         WHERE account_id = ?1
           AND expires_at IS NOT NULL
           AND expires_at <= CURRENT_TIMESTAMP",
    )
    .bind_refs(&account_id_binding)?
    .run()
    .await?;

    let bindings = [
        D1Type::Text(account_id),
        max_id
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null),
        since_id
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT rowid AS cursor_id, target_account_id, target_actor_uri
             FROM mutes
             WHERE account_id = ?1
               AND (?2 IS NULL OR rowid < ?2)
               AND (?3 IS NULL OR rowid > ?3)
             ORDER BY rowid DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<MuteEntryRow>()
}
