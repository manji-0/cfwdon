use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct EndorsedAccountEntryRow {
    pub(crate) cursor_id: i64,
    pub(crate) target_account_id: Option<String>,
    pub(crate) target_actor_uri: String,
}

pub(crate) async fn set_account_endorsement(
    db: &D1Database,
    account_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
    endorsed: bool,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        target_account_id.map(D1Type::Text).unwrap_or(D1Type::Null),
        D1Type::Text(target_actor_uri),
        D1Type::Integer(i32::from(endorsed)),
    ];
    db.prepare(
        "INSERT INTO account_social_metadata (
            account_id,
            target_account_id,
            target_actor_uri,
            endorsed,
            note,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            '',
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_actor_uri) DO UPDATE SET
            target_account_id = excluded.target_account_id,
            endorsed = excluded.endorsed,
            created_at = CASE
                WHEN excluded.endorsed != 0 THEN CURRENT_TIMESTAMP
                ELSE account_social_metadata.created_at
            END,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    prune_account_social_metadata(db, account_id, target_actor_uri).await
}

pub(crate) async fn set_account_note(
    db: &D1Database,
    account_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
    note: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        target_account_id.map(D1Type::Text).unwrap_or(D1Type::Null),
        D1Type::Text(target_actor_uri),
        D1Type::Text(note),
    ];
    db.prepare(
        "INSERT INTO account_social_metadata (
            account_id,
            target_account_id,
            target_actor_uri,
            endorsed,
            note,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            0,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_actor_uri) DO UPDATE SET
            target_account_id = excluded.target_account_id,
            note = excluded.note,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    prune_account_social_metadata(db, account_id, target_actor_uri).await
}

pub(crate) async fn list_endorsed_accounts_for_owner(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<EndorsedAccountEntryRow>> {
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
            "SELECT id AS cursor_id, target_account_id, target_actor_uri
             FROM account_social_metadata
             WHERE account_id = ?1
               AND endorsed != 0
               AND (?2 IS NULL OR id < ?2)
               AND (?3 IS NULL OR id > ?3)
             ORDER BY id DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<EndorsedAccountEntryRow>()
}

async fn prune_account_social_metadata(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_actor_uri)];
    db.prepare(
        "DELETE FROM account_social_metadata
         WHERE account_id = ?1
           AND target_actor_uri = ?2
           AND endorsed = 0
           AND note = ''",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}
