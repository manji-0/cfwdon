use crate::db_utils::{d1_results, json_string_array, sql_in_json_each};
use crate::tracked_d1::D1Database;
use cfwdon_domain::{RemoteStatus, StatusInteractionCounts};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use worker::{Result, d1::D1Type};

#[derive(Debug, Deserialize)]
struct StatusCountsRow {
    #[serde(default)]
    status_id: String,
    #[serde(default)]
    remote_status_id: String,
    favourites_count: u64,
    reblogs_count: u64,
    #[serde(default)]
    replies_count: u64,
}

impl StatusCountsRow {
    fn counts(&self) -> StatusInteractionCounts {
        StatusInteractionCounts {
            favourites: self.favourites_count,
            reblogs: self.reblogs_count,
            replies: self.replies_count,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct StatusCountsPreload {
    local: HashMap<String, StatusInteractionCounts>,
    remote: HashMap<String, StatusInteractionCounts>,
}

impl StatusCountsPreload {
    pub(crate) fn local_counts(&self, status_id: &str) -> Option<StatusInteractionCounts> {
        self.local.get(status_id).copied()
    }

    pub(crate) fn remote_counts(&self, status_id: &str) -> Option<StatusInteractionCounts> {
        self.remote.get(status_id).copied()
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.local.extend(other.local);
        self.remote.extend(other.remote);
    }
}

pub(crate) async fn load_local_status_counts(
    db: &D1Database,
    status_id: &str,
) -> Result<StatusInteractionCounts> {
    let status_id = D1Type::Text(status_id);
    let row = db
        .prepare(
            "SELECT favourites_count, reblogs_count, replies_count
             FROM status_counts
             WHERE status_id = ?1",
        )
        .bind_refs(&status_id)?
        .first::<StatusCountsRow>(None)
        .await?;

    Ok(row.map(|row| row.counts()).unwrap_or_default())
}

pub(crate) async fn load_local_status_counts_map(
    db: &D1Database,
    status_ids: &[String],
) -> Result<HashMap<String, StatusInteractionCounts>> {
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
        .map(|id| ((*id).clone(), StatusInteractionCounts::default()))
        .collect::<HashMap<_, _>>();

    let ids_json = json_string_array(&ids);
    let sql = format!(
        "SELECT status_id, favourites_count, reblogs_count, replies_count
         FROM status_counts
         WHERE status_id {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(ids_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;

    for row in d1_results::<StatusCountsRow>(&result)? {
        let value = row.counts();
        counts.insert(row.status_id, value);
    }
    Ok(counts)
}

pub(crate) async fn load_remote_status_counts(
    db: &D1Database,
    remote_status_id: &str,
) -> Result<StatusInteractionCounts> {
    let remote_status_id = D1Type::Text(remote_status_id);
    let row = db
        .prepare(
            "SELECT favourites_count, reblogs_count, replies_count
             FROM remote_status_counts
             WHERE remote_status_id = ?1",
        )
        .bind_refs(&remote_status_id)?
        .first::<StatusCountsRow>(None)
        .await?;

    Ok(row.map(|row| row.counts()).unwrap_or_default())
}

pub(crate) async fn load_remote_status_counts_map(
    db: &D1Database,
    remote_status_ids: &[String],
) -> Result<HashMap<String, StatusInteractionCounts>> {
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
        .map(|id| ((*id).clone(), StatusInteractionCounts::default()))
        .collect::<HashMap<_, _>>();

    let ids_json = json_string_array(&ids);
    let sql = format!(
        "SELECT remote_status_id, favourites_count, reblogs_count, replies_count
         FROM remote_status_counts
         WHERE remote_status_id {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(ids_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;

    for row in d1_results::<StatusCountsRow>(&result)? {
        let value = row.counts();
        counts.insert(row.remote_status_id, value);
    }
    Ok(counts)
}

pub(crate) async fn preload_status_counts(
    db: &D1Database,
    local_status_ids: &[String],
    remote_status_ids: &[String],
) -> Result<StatusCountsPreload> {
    let (local, remote) = futures_util::try_join!(
        load_local_status_counts_map(db, local_status_ids),
        load_remote_status_counts_map(db, remote_status_ids),
    )?;

    Ok(StatusCountsPreload { local, remote })
}

pub(crate) async fn preload_status_counts_for_remote_rows(
    db: &D1Database,
    local_status_ids: &[String],
    remote_statuses: &[&RemoteStatus],
) -> Result<StatusCountsPreload> {
    let mut seen = HashSet::new();
    let mut missing = Vec::new();
    let mut preload = StatusCountsPreload::default();
    for status in remote_statuses {
        if !seen.insert(status.id.as_str()) {
            continue;
        }
        match status.interaction_counts {
            Some(counts) => {
                preload.remote.insert(status.id.clone(), counts);
            }
            None => missing.push(status.id.clone()),
        }
    }
    let (local, fetched) = futures_util::try_join!(
        load_local_status_counts_map(db, local_status_ids),
        load_remote_status_counts_map(db, &missing),
    )?;
    preload.local = local;
    preload.remote.extend(fetched);
    Ok(preload)
}
