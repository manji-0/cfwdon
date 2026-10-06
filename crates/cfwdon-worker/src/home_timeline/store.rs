use super::{HomeTimelineCandidateSource, home_timeline_candidate_query};
use crate::db_utils::d1_results;
use crate::timelines::ResolvedTimelineCursor;
use crate::timelines::keep_timeline_page;
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use std::collections::HashSet;
use worker::Result;

pub(crate) const HOME_TIMELINE_CANDIDATE_SOURCE_LOCAL: &str = "local";
pub(crate) const HOME_TIMELINE_CANDIDATE_SOURCE_REMOTE: &str = "remote";

#[derive(Debug, Deserialize)]
pub(crate) struct HomeTimelineCandidateRow {
    pub(crate) source: String,
    pub(crate) status_id: String,
    pub(crate) timestamp: String,
}

pub(crate) async fn list_home_timeline_candidate_ids(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
    include_followed_tags: bool,
) -> Result<Vec<HomeTimelineCandidateRow>> {
    let (local_rows, remote_rows) = futures_util::try_join!(
        list_home_timeline_candidate_ids_for_source(
            db,
            viewer_account_id,
            cursor,
            limit,
            HomeTimelineCandidateSource::Local,
            include_followed_tags,
        ),
        list_home_timeline_candidate_ids_for_source(
            db,
            viewer_account_id,
            cursor,
            limit,
            HomeTimelineCandidateSource::Remote,
            include_followed_tags,
        ),
    )?;

    // Each side is already capped at `limit`, so the merged top `limit` is the
    // most the caller can use. Keeping more only inflates the hydration set.
    Ok(merge_home_timeline_candidate_rows(
        local_rows,
        remote_rows,
        limit,
        cursor.forward,
    ))
}

async fn list_home_timeline_candidate_ids_for_source(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
    source: HomeTimelineCandidateSource,
    include_followed_tags: bool,
) -> Result<Vec<HomeTimelineCandidateRow>> {
    let query = home_timeline_candidate_query(
        viewer_account_id,
        cursor,
        limit,
        source,
        include_followed_tags,
    );
    let result = db
        .prepare(&query.sql)
        .bind_refs(query.bindings.iter())?
        .all()
        .await?;
    d1_results::<HomeTimelineCandidateRow>(&result)
}

pub(crate) fn merge_home_timeline_candidate_rows(
    local_rows: Vec<HomeTimelineCandidateRow>,
    remote_rows: Vec<HomeTimelineCandidateRow>,
    limit: u32,
    forward: bool,
) -> Vec<HomeTimelineCandidateRow> {
    let mut rows = local_rows;
    rows.extend(remote_rows);
    rows.sort_by(|left, right| {
        right
            .timestamp
            .cmp(&left.timestamp)
            .then_with(|| right.status_id.cmp(&left.status_id))
    });

    let mut seen_status_ids = HashSet::new();
    rows.retain(|row| seen_status_ids.insert(row.status_id.clone()));
    keep_timeline_page(
        &mut rows,
        usize::try_from(limit).unwrap_or(usize::MAX),
        forward,
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::{HomeTimelineCandidateRow, merge_home_timeline_candidate_rows};

    fn row(source: &str, status_id: &str, timestamp: &str) -> HomeTimelineCandidateRow {
        HomeTimelineCandidateRow {
            source: source.to_owned(),
            status_id: status_id.to_owned(),
            timestamp: timestamp.to_owned(),
        }
    }

    #[test]
    fn merge_home_timeline_candidate_rows_orders_and_dedupes() {
        let merged = merge_home_timeline_candidate_rows(
            vec![
                row("local", "status-a", "2026-01-02T00:00:00Z"),
                row("local", "status-b", "2026-01-01T00:00:00Z"),
            ],
            vec![
                row("remote", "status-c", "2026-01-03T00:00:00Z"),
                row("local", "status-a", "2026-01-02T00:00:00Z"),
            ],
            10,
            false,
        );

        assert_eq!(
            merged
                .iter()
                .map(|row| row.status_id.as_str())
                .collect::<Vec<_>>(),
            vec!["status-c", "status-a", "status-b"]
        );
    }

    #[test]
    fn merge_home_timeline_candidate_rows_respects_limit() {
        let merged = merge_home_timeline_candidate_rows(
            vec![
                row("local", "status-a", "2026-01-03T00:00:00Z"),
                row("local", "status-b", "2026-01-02T00:00:00Z"),
            ],
            vec![row("remote", "status-c", "2026-01-01T00:00:00Z")],
            2,
            false,
        );

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].status_id, "status-a");
        assert_eq!(merged[1].status_id, "status-b");
    }
}
