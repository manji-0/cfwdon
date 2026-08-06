use super::{D1Database, Result, StatusRow, local_status_target_uri};
use serde::Deserialize;
use worker::d1::D1Type;

#[derive(Debug, Deserialize)]
pub(crate) struct BookmarkEntryRow {
    pub(crate) status_id: Option<String>,
    pub(crate) remote_status_id: Option<String>,
    pub(crate) created_at: String,
}

pub(crate) async fn upsert_bookmark_local_status(
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
        "INSERT INTO bookmarks (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            NULL,
            ?3,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            remote_status_id = NULL,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn upsert_bookmark_remote_status(
    db: &D1Database,
    account_id: &str,
    status: &super::RemoteStatusRow,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(status.object_uri.as_str()),
    ];

    db.prepare(
        "INSERT INTO bookmarks (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            NULL,
            ?2,
            ?3,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = NULL,
            remote_status_id = excluded.remote_status_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_bookmark_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "DELETE FROM bookmarks
         WHERE account_id = ?1
           AND target_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn is_local_status_bookmarked_by(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
) -> Result<bool> {
    is_bookmark_target_for_account(db, account_id, &local_status_target_uri(status)).await
}

pub(crate) async fn is_remote_status_bookmarked_by(
    db: &D1Database,
    account_id: &str,
    remote_status_id: &str,
) -> Result<bool> {
    let remote_status_id = D1Type::Text(remote_status_id);
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM bookmarks
             WHERE account_id = ?1
               AND remote_status_id = ?2
             LIMIT 1",
        )
        .bind_refs(&[account_id, remote_status_id])?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
}

async fn is_bookmark_target_for_account(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM bookmarks
             WHERE account_id = ?1
               AND target_uri = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
}

pub(crate) async fn list_bookmarks_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<BookmarkEntryRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT status_id, remote_status_id, created_at
             FROM bookmarks
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    crate::d1_results::<BookmarkEntryRow>(&result)
}
