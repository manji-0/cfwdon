use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use worker::{D1Database, Result, d1::D1Type};

#[derive(Debug, Deserialize)]
struct StatusCountsRow {
    #[serde(default)]
    status_id: String,
    #[serde(default)]
    remote_status_id: String,
    favourites_count: u64,
    reblogs_count: u64,
}

#[derive(Debug, Default)]
pub(crate) struct StatusCountsPreload {
    local: HashMap<String, (u64, u64)>,
    remote: HashMap<String, (u64, u64)>,
}

impl StatusCountsPreload {
    pub(crate) fn local_counts(&self, status_id: &str) -> Option<(u64, u64)> {
        self.local.get(status_id).copied()
    }

    pub(crate) fn remote_counts(&self, status_id: &str) -> Option<(u64, u64)> {
        self.remote.get(status_id).copied()
    }
}

pub(crate) async fn load_local_status_counts(
    db: &D1Database,
    status_id: &str,
) -> Result<(u64, u64)> {
    let status_id = D1Type::Text(status_id);
    let row = db
        .prepare(
            "SELECT favourites_count, reblogs_count
             FROM status_counts
             WHERE status_id = ?1",
        )
        .bind_refs(&status_id)?
        .first::<StatusCountsRow>(None)
        .await?;

    Ok(row
        .map(|row| (row.favourites_count, row.reblogs_count))
        .unwrap_or((0, 0)))
}

pub(crate) async fn load_local_status_counts_map(
    db: &D1Database,
    status_ids: &[String],
) -> Result<HashMap<String, (u64, u64)>> {
    let mut seen = HashSet::new();
    let ids = status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut counts = ids
        .iter()
        .map(|id| ((*id).clone(), (0, 0)))
        .collect::<HashMap<_, _>>();

    let placeholders = (1..=ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT status_id, favourites_count, reblogs_count
         FROM status_counts
         WHERE status_id IN ({placeholders})"
    );
    let bindings = ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    for row in result.results::<StatusCountsRow>()? {
        counts.insert(row.status_id, (row.favourites_count, row.reblogs_count));
    }
    Ok(counts)
}

pub(crate) async fn load_remote_status_counts(
    db: &D1Database,
    remote_status_id: &str,
) -> Result<(u64, u64)> {
    let remote_status_id = D1Type::Text(remote_status_id);
    let row = db
        .prepare(
            "SELECT favourites_count, reblogs_count
             FROM remote_status_counts
             WHERE remote_status_id = ?1",
        )
        .bind_refs(&remote_status_id)?
        .first::<StatusCountsRow>(None)
        .await?;

    Ok(row
        .map(|row| (row.favourites_count, row.reblogs_count))
        .unwrap_or((0, 0)))
}

pub(crate) async fn load_remote_status_counts_map(
    db: &D1Database,
    remote_status_ids: &[String],
) -> Result<HashMap<String, (u64, u64)>> {
    let mut seen = HashSet::new();
    let ids = remote_status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut counts = ids
        .iter()
        .map(|id| ((*id).clone(), (0, 0)))
        .collect::<HashMap<_, _>>();

    let placeholders = (1..=ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT remote_status_id, favourites_count, reblogs_count
         FROM remote_status_counts
         WHERE remote_status_id IN ({placeholders})"
    );
    let bindings = ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    for row in result.results::<StatusCountsRow>()? {
        counts.insert(
            row.remote_status_id,
            (row.favourites_count, row.reblogs_count),
        );
    }
    Ok(counts)
}

pub(crate) async fn preload_status_counts(
    db: &D1Database,
    local_status_ids: &[String],
    remote_status_ids: &[String],
) -> Result<StatusCountsPreload> {
    Ok(StatusCountsPreload {
        local: load_local_status_counts_map(db, local_status_ids).await?,
        remote: load_remote_status_counts_map(db, remote_status_ids).await?,
    })
}
