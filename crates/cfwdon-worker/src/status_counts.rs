use serde::Deserialize;
use worker::{D1Database, Result, d1::D1Type};

#[derive(Debug, Deserialize)]
struct StatusCountsRow {
    favourites_count: u64,
    reblogs_count: u64,
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
