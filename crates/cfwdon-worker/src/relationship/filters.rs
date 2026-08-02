use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
pub(crate) async fn upsert_block(
    db: &D1Database,
    blocker_account_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(blocker_account_id),
        match target_account_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(target_actor_uri),
    ];

    db.prepare(
        "INSERT INTO blocks (
            blocker_account_id,
            target_account_id,
            target_actor_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(blocker_account_id, target_actor_uri) DO UPDATE SET
            target_account_id = excluded.target_account_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_block_by_target(
    db: &D1Database,
    blocker_account_id: &str,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(blocker_account_id),
        D1Type::Text(target_actor_uri),
    ];

    db.prepare(
        "DELETE FROM blocks
         WHERE blocker_account_id = ?1
           AND target_actor_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn upsert_mute(
    db: &D1Database,
    account_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
    notifications: bool,
    expires_at: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        match target_account_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(target_actor_uri),
        D1Type::Integer(if notifications { 1 } else { 0 }),
        match expires_at {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];

    db.prepare(
        "INSERT INTO mutes (
            account_id,
            target_account_id,
            target_actor_uri,
            notifications,
            expires_at,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_actor_uri) DO UPDATE SET
            target_account_id = excluded.target_account_id,
            notifications = excluded.notifications,
            expires_at = excluded.expires_at,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_mute_by_target(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_actor_uri)];
    db.prepare(
        "DELETE FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}
