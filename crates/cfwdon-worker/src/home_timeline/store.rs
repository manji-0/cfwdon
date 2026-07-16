use super::{
    D1Database, ResolvedTimelineCursor, Result, home_timeline_candidate_bindings,
    home_timeline_candidate_merge_limit, home_timeline_local_branch_count,
    home_timeline_local_candidate_sql, home_timeline_remote_branch_count,
    home_timeline_remote_candidate_sql,
};
use serde::Deserialize;
use std::collections::HashSet;
use worker::d1::D1Type;

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
        list_home_timeline_candidate_ids_for_sql(
            db,
            home_timeline_local_candidate_sql(include_followed_tags),
            viewer_account_id,
            cursor,
            limit,
            home_timeline_local_branch_count(include_followed_tags),
        ),
        list_home_timeline_candidate_ids_for_sql(
            db,
            home_timeline_remote_candidate_sql(include_followed_tags),
            viewer_account_id,
            cursor,
            limit,
            home_timeline_remote_branch_count(include_followed_tags),
        ),
    )?;

    Ok(merge_home_timeline_candidate_rows(
        local_rows,
        remote_rows,
        home_timeline_candidate_merge_limit(limit),
    ))
}

pub(crate) async fn account_has_followed_tags(db: &D1Database, account_id: &str) -> Result<bool> {
    let account_id = D1Type::Text(account_id);
    Ok(db
        .prepare(
            "SELECT tag_name
             FROM followed_tags
             WHERE account_id = ?1
             LIMIT 1",
        )
        .bind_refs(&account_id)?
        .first::<serde_json::Value>(None)
        .await?
        .is_some())
}

async fn list_home_timeline_candidate_ids_for_sql(
    db: &D1Database,
    sql: &str,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
    branch_count: u32,
) -> Result<Vec<HomeTimelineCandidateRow>> {
    let bindings = home_timeline_candidate_bindings(viewer_account_id, cursor, limit, branch_count);
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;
    result.results::<HomeTimelineCandidateRow>()
}

fn merge_home_timeline_candidate_rows(
    local_rows: Vec<HomeTimelineCandidateRow>,
    remote_rows: Vec<HomeTimelineCandidateRow>,
    limit: u32,
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
    let keep = usize::try_from(limit).unwrap_or(usize::MAX);
    if rows.len() > keep {
        rows.truncate(keep);
    }
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
        );

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].status_id, "status-a");
        assert_eq!(merged[1].status_id, "status-b");
    }
}
