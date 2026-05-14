use super::{D1Database, Result, generate_entity_id};
use worker::d1::D1Type;

pub(crate) async fn insert_status_edit_snapshot(
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
        "INSERT INTO status_edits (
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

pub(crate) async fn list_status_edit_snapshots(
    db: &D1Database,
    status_id: &str,
) -> Result<Vec<serde_json::Value>> {
    let status_id = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT snapshot_json
             FROM status_edits
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

pub(crate) async fn load_status_updated_at(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<String>> {
    let status_id = D1Type::Text(status_id);
    let row = db
        .prepare(
            "SELECT updated_at
             FROM statuses
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
