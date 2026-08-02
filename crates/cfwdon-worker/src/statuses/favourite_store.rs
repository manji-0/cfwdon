use super::{
    AppConfig, RemoteStatusRow, StatusRow, local_status_target_uri,
    publish_local_status_interaction_notification_soft, send_push_notification,
};
use cfwdon_domain::LocalAccount;
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{Env, Result};

use crate::D1Database;
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

#[derive(Debug, Deserialize)]
struct InteractionAccountIdRow {
    account_id: String,
}

#[derive(Debug, Deserialize)]
struct InteractionActorUriRow {
    remote_actor_uri: String,
}

pub(crate) async fn upsert_favourite_local_status(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    actor: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    let account_id = actor.id();
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

    let _ = send_push_notification(
        db,
        config,
        &status.account_id,
        "favourite",
        serde_json::json!({
            "account_id": account_id,
            "status_id": status.id,
        }),
    )
    .await;

    let _ = publish_local_status_interaction_notification_soft(
        env,
        db,
        config,
        &status.account_id,
        actor,
        "favourite",
        status,
    )
    .await;

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
            "SELECT 1 AS found
             FROM favourites
             WHERE account_id = ?1
               AND remote_status_id = ?2
             LIMIT 1",
        )
        .bind_refs(&[account_id, remote_status_id])?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
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

pub(crate) async fn list_local_favourite_account_ids_for_status(
    db: &D1Database,
    status_id: &str,
    limit: u32,
) -> Result<Vec<String>> {
    list_interaction_account_ids(
        db,
        "SELECT account_id
         FROM favourites
         WHERE status_id = ?1
         ORDER BY created_at DESC
         LIMIT ?2",
        status_id,
        limit,
    )
    .await
}

pub(crate) async fn list_local_favourite_account_ids_for_remote_status(
    db: &D1Database,
    remote_status_id: &str,
    limit: u32,
) -> Result<Vec<String>> {
    list_interaction_account_ids(
        db,
        "SELECT account_id
         FROM favourites
         WHERE remote_status_id = ?1
         ORDER BY created_at DESC
         LIMIT ?2",
        remote_status_id,
        limit,
    )
    .await
}

pub(crate) async fn list_remote_favourite_actor_uris_for_status(
    db: &D1Database,
    status_id: &str,
    limit: u32,
) -> Result<Vec<String>> {
    list_interaction_actor_uris(
        db,
        "SELECT remote_actor_uri
         FROM remote_favourites
         WHERE status_id = ?1
         ORDER BY created_at DESC
         LIMIT ?2",
        status_id,
        limit,
    )
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
            "SELECT 1 AS found
             FROM favourites
             WHERE account_id = ?1
               AND target_uri = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
}

async fn list_interaction_account_ids(
    db: &D1Database,
    sql: &str,
    target_id: &str,
    limit: u32,
) -> Result<Vec<String>> {
    let bindings = [D1Type::Text(target_id), D1Type::Integer(limit as i32)];
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<InteractionAccountIdRow>()?
        .into_iter()
        .map(|row| row.account_id)
        .collect())
}

async fn list_interaction_actor_uris(
    db: &D1Database,
    sql: &str,
    target_id: &str,
    limit: u32,
) -> Result<Vec<String>> {
    let bindings = [D1Type::Text(target_id), D1Type::Integer(limit as i32)];
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<InteractionActorUriRow>()?
        .into_iter()
        .map(|row| row.remote_actor_uri)
        .collect())
}
