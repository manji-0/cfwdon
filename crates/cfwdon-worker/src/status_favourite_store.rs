use crate::{RemoteStatusRow, StatusRow, count_rows, local_status_target_uri};
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct FavouriteEntryRow {
    pub(crate) status_id: Option<String>,
    pub(crate) remote_status_id: Option<String>,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InteractionActivityRow {
    pub(crate) ap_activity_id: Option<String>,
}

pub(crate) async fn upsert_favourite_local_status(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
) -> Result<()> {
    let target_uri = local_status_target_uri(status);
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(target_uri.as_str()),
    ];

    db.prepare(
        "INSERT INTO favourites (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            ap_activity_id,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            NULL,
            ?3,
            NULL,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            remote_status_id = NULL,
            ap_activity_id = NULL,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn upsert_favourite_remote_status(
    db: &D1Database,
    account_id: &str,
    status: &RemoteStatusRow,
    ap_activity_id: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(status.object_uri.as_str()),
        match ap_activity_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];

    db.prepare(
        "INSERT INTO favourites (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            ap_activity_id,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            NULL,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = NULL,
            remote_status_id = excluded.remote_status_id,
            ap_activity_id = excluded.ap_activity_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_favourite_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "DELETE FROM favourites
         WHERE account_id = ?1
           AND target_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn count_local_status_favourites(db: &D1Database, status_id: &str) -> Result<u64> {
    Ok(count_rows(
        db,
        "SELECT COUNT(*) AS count FROM favourites WHERE status_id = ?1",
        status_id,
    )
    .await?
        + count_rows(
            db,
            "SELECT COUNT(*) AS count FROM remote_favourites WHERE status_id = ?1",
            status_id,
        )
        .await?)
}

pub(crate) async fn count_remote_status_favourites(
    db: &D1Database,
    remote_status_id: &str,
) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count FROM favourites WHERE remote_status_id = ?1",
        remote_status_id,
    )
    .await
}

pub(crate) async fn is_local_status_favourited_by(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
) -> Result<bool> {
    is_favourite_target_for_account(db, account_id, &local_status_target_uri(status)).await
}

pub(crate) async fn is_remote_status_favourited_by(
    db: &D1Database,
    account_id: &str,
    remote_status_id: &str,
) -> Result<bool> {
    let remote_status_id = D1Type::Text(remote_status_id);
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM favourites
             WHERE account_id = ?1
               AND remote_status_id = ?2",
        )
        .bind_refs(&[account_id, remote_status_id])?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

pub(crate) async fn list_favourites_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<FavouriteEntryRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT status_id, remote_status_id, created_at
             FROM favourites
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<FavouriteEntryRow>()
}

pub(crate) async fn find_favourite_activity_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<Option<InteractionActivityRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "SELECT ap_activity_id
         FROM favourites
         WHERE account_id = ?1
           AND target_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<InteractionActivityRow>(None)
    .await
}

async fn is_favourite_target_for_account(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM favourites
             WHERE account_id = ?1
               AND target_uri = ?2",
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
