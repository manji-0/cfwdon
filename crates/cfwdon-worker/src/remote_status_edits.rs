use crate::{D1Database, Result, generate_entity_id};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use worker::d1::D1Type;

#[derive(Debug, Deserialize)]
struct RemoteStatusUpdatedAtRow {
    id: String,
    updated_at: String,
}

#[derive(Debug, Default)]
pub(crate) struct RemoteStatusEditUpdatedAtPreload {
    updated_at_by_status_id: HashMap<String, String>,
}

impl RemoteStatusEditUpdatedAtPreload {
    pub(crate) fn updated_at(&self, status_id: &str) -> Option<&str> {
        self.updated_at_by_status_id
            .get(status_id)
            .map(String::as_str)
    }
}

pub(crate) async fn insert_remote_status_edit_snapshot(
    db: &D1Database,
    status_id: &str,
    snapshot_json: &str,
    created_at: &str,
) -> Result<()> {
    let edit_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(edit_id.as_str()),
        D1Type::Text(status_id),
        D1Type::Text(snapshot_json),
        D1Type::Text(created_at),
    ];
    db.prepare(
        "INSERT INTO remote_status_edits (
            id,
            status_id,
            snapshot_json,
            created_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn list_remote_status_edit_snapshots(
    db: &D1Database,
    status_id: &str,
) -> Result<Vec<serde_json::Value>> {
    let status_id = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT snapshot_json
             FROM remote_status_edits
             WHERE status_id = ?1
             ORDER BY created_at DESC, id DESC",
        )
        .bind_refs(&status_id)?
        .all()
        .await?;

    Ok(result
        .results::<serde_json::Value>()?
        .into_iter()
        .filter_map(|value| {
            value
                .get("snapshot_json")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter_map(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .map(crate::normalize_status_history_entry)
        .collect())
}

pub(crate) async fn has_remote_status_edit_snapshots(
    db: &D1Database,
    status_id: &str,
) -> Result<bool> {
    let status_id = D1Type::Text(status_id);
    let row = db
        .prepare(
            "SELECT 1 AS present
             FROM remote_status_edits
             WHERE status_id = ?1
             LIMIT 1",
        )
        .bind_refs(&status_id)?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.is_some())
}

pub(crate) async fn load_remote_status_updated_at(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<String>> {
    let status_id = D1Type::Text(status_id);
    let row = db
        .prepare(
            "SELECT updated_at
             FROM remote_statuses
             WHERE id = ?1
             LIMIT 1",
        )
        .bind_refs(&status_id)?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.and_then(|value| {
        value
            .get("updated_at")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    }))
}

pub(crate) async fn preload_remote_status_edit_updated_at(
    db: &D1Database,
    status_ids: &[String],
) -> Result<RemoteStatusEditUpdatedAtPreload> {
    let mut seen = HashSet::new();
    let ids = status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(RemoteStatusEditUpdatedAtPreload::default());
    }

    let placeholders = (1..=ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT rs.id, rs.updated_at
         FROM remote_statuses rs
         WHERE rs.id IN ({placeholders})
           AND EXISTS (
               SELECT 1
               FROM remote_status_edits rse
               WHERE rse.status_id = rs.id
               LIMIT 1
           )"
    );
    let bindings = ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
    let updated_at_by_status_id = result
        .results::<RemoteStatusUpdatedAtRow>()?
        .into_iter()
        .map(|row| (row.id, row.updated_at))
        .collect::<HashMap<_, _>>();

    Ok(RemoteStatusEditUpdatedAtPreload {
        updated_at_by_status_id,
    })
}
