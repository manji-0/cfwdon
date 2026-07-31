//! Candidate-id queries backing the home timeline.
//!
//! The queries are assembled per request rather than kept as constants so the
//! pagination cursor can appear as a plain range constraint. Expressing it as
//! `?n IS NULL OR ts < ?n` reads well but costs a full scan: SQLite cannot use a
//! disjunction as an index range constraint, so every page started at the newest
//! row and discarded everything above the cursor, making deep pages progressively
//! slower. Emitting the bound only when it exists lets the index seek straight to
//! it.

use crate::ResolvedTimelineCursor;
use worker::d1::D1Type;

/// One `UNION` arm of a candidate query.
struct CandidateBranch {
    source: &'static str,
    /// `FROM` and the branch's own `WHERE` conditions, cursor bounds excluded.
    from_and_where: &'static str,
    timestamp_column: &'static str,
    id_column: &'static str,
}

const LOCAL_OWN_STATUSES: CandidateBranch = CandidateBranch {
    source: "local",
    from_and_where: "FROM statuses s
                    WHERE s.account_id = ?1",
    timestamp_column: "s.created_at",
    id_column: "s.id",
};

const LOCAL_FOLLOWED_ACCOUNTS: CandidateBranch = CandidateBranch {
    source: "local",
    from_and_where: "FROM follows f
                    JOIN statuses s
                      ON s.account_id = f.target_account_id
                    WHERE f.follower_account_id = ?1
                      AND f.state = 'accepted'
                      AND s.visibility IN ('public', 'unlisted', 'private')",
    timestamp_column: "s.created_at",
    id_column: "s.id",
};

const LOCAL_FOLLOWED_TAGS: CandidateBranch = CandidateBranch {
    source: "local",
    from_and_where: "FROM followed_tags ft
                    JOIN status_hashtags h
                      ON h.tag = ft.tag_name
                    JOIN statuses s
                      ON s.id = h.status_id
                    WHERE ft.account_id = ?1
                      AND s.visibility = 'public'",
    timestamp_column: "s.created_at",
    id_column: "s.id",
};

const REMOTE_FOLLOWED_ACCOUNTS: CandidateBranch = CandidateBranch {
    source: "remote",
    from_and_where: "FROM follows f
                    JOIN remote_statuses rs
                      ON rs.actor_uri = f.target_actor_uri
                    WHERE f.follower_account_id = ?1
                      AND f.state = 'accepted'
                      AND rs.visibility IN ('public', 'unlisted', 'private')",
    timestamp_column: "rs.published_at",
    id_column: "rs.id",
};

const REMOTE_FOLLOWED_TAGS: CandidateBranch = CandidateBranch {
    source: "remote",
    from_and_where: "FROM followed_tags ft
                    JOIN remote_status_hashtags h
                      ON h.tag = ft.tag_name
                    JOIN remote_statuses rs
                      ON rs.id = h.status_id
                    WHERE ft.account_id = ?1
                      AND rs.visibility = 'public'",
    timestamp_column: "rs.published_at",
    id_column: "rs.id",
};

/// Which statuses a candidate query draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HomeTimelineCandidateSource {
    Local,
    Remote,
}

impl HomeTimelineCandidateSource {
    fn branches(self, include_followed_tags: bool) -> &'static [CandidateBranch] {
        match (self, include_followed_tags) {
            (Self::Local, false) => &[LOCAL_OWN_STATUSES, LOCAL_FOLLOWED_ACCOUNTS],
            (Self::Local, true) => &[
                LOCAL_OWN_STATUSES,
                LOCAL_FOLLOWED_ACCOUNTS,
                LOCAL_FOLLOWED_TAGS,
            ],
            (Self::Remote, false) => &[REMOTE_FOLLOWED_ACCOUNTS],
            (Self::Remote, true) => &[REMOTE_FOLLOWED_ACCOUNTS, REMOTE_FOLLOWED_TAGS],
        }
    }
}

/// A candidate query with its bindings, which vary with the cursor.
pub(crate) struct HomeTimelineCandidateQuery<'a> {
    pub(crate) sql: String,
    pub(crate) bindings: Vec<D1Type<'a>>,
}

/// Bind slots assigned to whichever cursor bounds are present.
#[derive(Default)]
struct CursorSlots {
    /// `(timestamp slot, id slot)` for the upper bound.
    max: Option<(usize, usize)>,
    /// `(timestamp slot, id slot)` for the lower bound.
    min: Option<(usize, usize)>,
}

/// Builds the candidate query for one source.
///
/// `limit` caps each branch and the merged result; see
/// [`super::merge_home_timeline_candidate_rows`] for why one cap covers both.
pub(crate) fn home_timeline_candidate_query<'a>(
    viewer_account_id: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
    source: HomeTimelineCandidateSource,
    include_followed_tags: bool,
) -> HomeTimelineCandidateQuery<'a> {
    // `?1` is always the viewer; cursor bounds and then the limit follow.
    let mut bindings = vec![D1Type::Text(viewer_account_id)];
    let mut slots = CursorSlots::default();

    // A resolved cursor always carries both a timestamp and an id: the timestamp
    // is looked up from the id, and a request whose id did not resolve is
    // answered with an empty page before reaching this point.
    if let (Some(timestamp), Some(id)) = (cursor.max_timestamp.as_deref(), cursor.max_id.as_deref())
    {
        bindings.push(D1Type::Text(timestamp));
        bindings.push(D1Type::Text(id));
        slots.max = Some((bindings.len() - 1, bindings.len()));
    }
    if let (Some(timestamp), Some(id)) = (cursor.min_timestamp.as_deref(), cursor.min_id.as_deref())
    {
        bindings.push(D1Type::Text(timestamp));
        bindings.push(D1Type::Text(id));
        slots.min = Some((bindings.len() - 1, bindings.len()));
    }
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();

    let branches = source
        .branches(include_followed_tags)
        .iter()
        .map(|branch| branch_sql(branch, &slots, limit_slot))
        .collect::<Vec<_>>()
        .join("\n\n                UNION\n\n                ");

    let sql = format!(
        "SELECT source, status_id, timestamp
             FROM (
                {branches}
             )
             ORDER BY timestamp DESC, status_id DESC
             LIMIT ?{limit_slot}"
    );

    HomeTimelineCandidateQuery { sql, bindings }
}

fn branch_sql(branch: &CandidateBranch, slots: &CursorSlots, limit_slot: usize) -> String {
    let CandidateBranch {
        source,
        from_and_where,
        timestamp_column,
        id_column,
    } = branch;
    let cursor_predicates = cursor_predicates(timestamp_column, id_column, slots);

    format!(
        "SELECT source, status_id, timestamp
                FROM (
                    SELECT '{source}' AS source, {id_column} AS status_id, {timestamp_column} AS timestamp
                    {from_and_where}{cursor_predicates}
                    ORDER BY {timestamp_column} DESC, {id_column} DESC
                    LIMIT ?{limit_slot}
                )"
    )
}

/// Renders the cursor bounds as seekable range constraints.
///
/// Each bound leads with a bare comparison on the timestamp so the index can
/// seek to it, then breaks ties on the id. Writing the pair as a single
/// disjunction instead would leave nothing for the planner to seek on.
fn cursor_predicates(timestamp_column: &str, id_column: &str, slots: &CursorSlots) -> String {
    let mut predicates = String::new();
    if let Some((timestamp_slot, id_slot)) = slots.max {
        predicates.push_str(&format!(
            "
                      AND {timestamp_column} <= ?{timestamp_slot}
                      AND ({timestamp_column} < ?{timestamp_slot} OR {id_column} < ?{id_slot})"
        ));
    }
    if let Some((timestamp_slot, id_slot)) = slots.min {
        predicates.push_str(&format!(
            "
                      AND {timestamp_column} >= ?{timestamp_slot}
                      AND ({timestamp_column} > ?{timestamp_slot} OR {id_column} > ?{id_slot})"
        ));
    }
    predicates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cursor() -> ResolvedTimelineCursor {
        ResolvedTimelineCursor {
            max_timestamp: None,
            max_id: None,
            min_timestamp: None,
            min_id: None,
        }
    }

    fn max_cursor() -> ResolvedTimelineCursor {
        ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: None,
            min_id: None,
        }
    }

    #[test]
    fn open_cursor_query_binds_only_viewer_and_limit() {
        let cursor = empty_cursor();
        let query = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Local,
            false,
        );

        assert_eq!(query.bindings.len(), 2);
        assert!(matches!(query.bindings[0], D1Type::Text("viewer")));
        assert!(matches!(query.bindings[1], D1Type::Integer(20)));
        // No bound to seek to, so no cursor predicate should be emitted.
        assert!(!query.sql.contains("<= ?"));
        assert!(!query.sql.contains(">= ?"));
        assert!(query.sql.contains("LIMIT ?2"));
    }

    #[test]
    fn max_cursor_query_emits_a_seekable_upper_bound() {
        let cursor = max_cursor();
        let query = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Local,
            false,
        );

        assert_eq!(query.bindings.len(), 4);
        assert!(matches!(query.bindings[0], D1Type::Text("viewer")));
        assert!(matches!(
            query.bindings[1],
            D1Type::Text("2026-01-02T00:00:00Z")
        ));
        assert!(matches!(query.bindings[2], D1Type::Text("status-max")));
        assert!(matches!(query.bindings[3], D1Type::Integer(20)));
        // The bare comparison is what the index can seek on.
        assert!(query.sql.contains("s.created_at <= ?2"));
        assert!(query.sql.contains("(s.created_at < ?2 OR s.id < ?3)"));
        // The old form defeated the seek and must not come back.
        assert!(!query.sql.contains("IS NULL"));
        assert!(query.sql.contains("LIMIT ?4"));
    }

    #[test]
    fn both_cursor_bounds_get_distinct_slots() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
            min_id: Some("status-min".to_owned()),
        };
        let query = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Remote,
            false,
        );

        assert_eq!(query.bindings.len(), 6);
        assert!(query.sql.contains("rs.published_at <= ?2"));
        assert!(query.sql.contains("rs.published_at >= ?4"));
        assert!(query.sql.contains("(rs.published_at > ?4 OR rs.id > ?5)"));
        assert!(query.sql.contains("LIMIT ?6"));
    }

    #[test]
    fn every_bind_slot_is_referenced_by_the_sql() {
        for include_followed_tags in [false, true] {
            for source in [
                HomeTimelineCandidateSource::Local,
                HomeTimelineCandidateSource::Remote,
            ] {
                for cursor in [empty_cursor(), max_cursor()] {
                    let query = home_timeline_candidate_query(
                        "viewer",
                        &cursor,
                        20,
                        source,
                        include_followed_tags,
                    );
                    for slot in 1..=query.bindings.len() {
                        assert!(
                            query.sql.contains(&format!("?{slot}")),
                            "slot ?{slot} is bound but never referenced: {}",
                            query.sql
                        );
                    }
                    // A slot past the end would mean a binding is missing.
                    let past_end = query.bindings.len() + 1;
                    assert!(!query.sql.contains(&format!("?{past_end}")));
                }
            }
        }
    }

    #[test]
    fn followed_tag_branches_are_only_added_when_requested() {
        let cursor = empty_cursor();
        let without = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Local,
            false,
        );
        let with = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Local,
            true,
        );

        assert!(!without.sql.contains("followed_tags"));
        assert!(with.sql.contains("followed_tags"));
        assert_eq!(without.sql.matches("UNION").count(), 1);
        assert_eq!(with.sql.matches("UNION").count(), 2);
    }

    #[test]
    fn each_branch_and_the_merge_share_the_limit_slot() {
        let cursor = empty_cursor();
        let query = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Local,
            true,
        );

        // Three branches plus the outer merge.
        assert_eq!(query.sql.matches("LIMIT ?2").count(), 4);
    }
}
