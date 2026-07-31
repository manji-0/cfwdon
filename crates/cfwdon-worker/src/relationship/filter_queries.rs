use serde::Deserialize;
use std::collections::HashSet;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct MuteRow {
    pub(crate) notifications: i32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MuteEntryRow {
    pub(crate) cursor_id: i64,
    pub(crate) target_account_id: Option<String>,
    pub(crate) target_actor_uri: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BlockEntryRow {
    pub(crate) cursor_id: i64,
    pub(crate) target_account_id: Option<String>,
    pub(crate) target_actor_uri: String,
}

#[derive(Debug, Deserialize)]
struct MutedActorUriRow {
    target_actor_uri: String,
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
            "SELECT 1 AS found
             FROM blocks
             WHERE blocker_account_id = ?1
               AND target_actor_uri = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
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
        "SELECT notifications
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

pub(crate) async fn list_active_muted_actor_uris(
    db: &D1Database,
    account_id: &str,
    target_actor_uris: &[String],
) -> Result<HashSet<String>> {
    let mut seen = HashSet::new();
    let target_actor_uris = target_actor_uris
        .iter()
        .filter(|uri| seen.insert(uri.as_str()))
        .collect::<Vec<_>>();
    if target_actor_uris.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders = (2..=(target_actor_uris.len() + 1))
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut bindings = Vec::with_capacity(target_actor_uris.len() + 1);
    bindings.push(D1Type::Text(account_id));
    bindings.extend(
        target_actor_uris
            .iter()
            .map(|uri| D1Type::Text(uri.as_str())),
    );

    let delete_sql = format!(
        "DELETE FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri IN ({placeholders})
           AND expires_at IS NOT NULL
           AND expires_at <= CURRENT_TIMESTAMP"
    );
    db.prepare(&delete_sql)
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    let select_sql = format!(
        "SELECT target_actor_uri
         FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri IN ({placeholders})"
    );
    let result = db
        .prepare(&select_sql)
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<MutedActorUriRow>()?
        .into_iter()
        .map(|row| row.target_actor_uri)
        .collect())
}

/// Every actor URI the account currently mutes.
///
/// Timelines used to ask "which of these N candidate actors are muted?", which
/// forced the candidates' authors to be known first and so serialized behind the
/// account lookup. A viewer's mute list is small and candidate-independent, so
/// fetching it whole lets the check run as a plain set membership test and start
/// as early as authentication.
///
/// Unlike [`list_active_muted_actor_uris`] this does not delete expired rows: a
/// timeline read should not write. Expired mutes are filtered out here and are
/// still collected by [`list_mutes_for_account`].
pub(crate) async fn list_active_muted_actor_uris_for_account(
    db: &D1Database,
    account_id: &str,
) -> Result<HashSet<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT target_actor_uri
             FROM mutes
             WHERE account_id = ?1
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<MutedActorUriRow>()?
        .into_iter()
        .map(|row| row.target_actor_uri)
        .collect())
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

pub(crate) async fn list_blocks_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<BlockEntryRow>> {
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
             FROM blocks
             WHERE blocker_account_id = ?1
               AND (?2 IS NULL OR rowid < ?2)
               AND (?3 IS NULL OR rowid > ?3)
             ORDER BY rowid DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<BlockEntryRow>()
}
