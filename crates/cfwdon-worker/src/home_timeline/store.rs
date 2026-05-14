use super::{
    D1Database, HOME_TIMELINE_CANDIDATE_SQL, ResolvedTimelineCursor, Result,
    home_timeline_candidate_bindings,
};
use serde::Deserialize;

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
) -> Result<Vec<HomeTimelineCandidateRow>> {
    let bindings = home_timeline_candidate_bindings(viewer_account_id, cursor, limit);
    let result = db
        .prepare(HOME_TIMELINE_CANDIDATE_SQL)
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<HomeTimelineCandidateRow>()
}
