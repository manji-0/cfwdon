//! Candidate-id queries backing the home timeline.
//!
//! The queries are assembled per request rather than kept as constants so the
//! pagination cursor can appear as a plain range constraint. Expressing it as
//! `?n IS NULL OR ts < ?n` reads well but costs a full scan: SQLite cannot use a
//! disjunction as an index range constraint, so every page started at the newest
//! row and discarded everything above the cursor, making deep pages progressively
//! slower. Emitting the bound only when it exists lets the index seek straight to
//! it.
//!
//! The remote follow branch is the exception to a single ordered range. A join of
//! every followed actor ordered by `published_at` reads each actor's whole history
//! before `LIMIT`. That branch instead takes `limit` rows per actor, in index
//! order, and the branch `ORDER BY` merges those slices.
use crate::timelines::{
    ResolvedTimelineCursor, ResolvedTimelineCursorSlots, append_resolved_timeline_cursor_bindings,
    seekable_resolved_timeline_cursor_predicates,
};
use worker::d1::D1Type;

/// One `UNION` arm of a candidate query.
struct CandidateBranch {
    source: &'static str,
    /// `FROM` and the branch's own `WHERE` conditions, cursor bounds excluded.
    /// Unused when `per_actor_limit` is set.
    from_and_where: &'static str,
    timestamp_column: &'static str,
    id_column: &'static str,
    /// Cap each followed actor, then let this branch merge those slices.
    per_actor_limit: bool,
}

const LOCAL_OWN_STATUSES: CandidateBranch = CandidateBranch {
    source: "local",
    from_and_where: "FROM statuses s
                    WHERE s.account_id = ?1",
    timestamp_column: "s.created_at",
    id_column: "s.id",
    per_actor_limit: false,
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
    per_actor_limit: false,
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
    per_actor_limit: false,
};

const REMOTE_FOLLOWED_ACCOUNTS: CandidateBranch = CandidateBranch {
    source: "remote",
    from_and_where: "",
    timestamp_column: "rs.published_at",
    id_column: "rs.id",
    per_actor_limit: true,
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
    per_actor_limit: false,
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
type CursorSlots = ResolvedTimelineCursorSlots;

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
    let dir = cursor.order_direction();
    // `?1` is always the viewer; cursor bounds and then the limit follow.
    let mut bindings = vec![D1Type::Text(viewer_account_id)];
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();

    let branches = source
        .branches(include_followed_tags)
        .iter()
        .map(|branch| branch_sql(branch, &slots, limit_slot, dir))
        .collect::<Vec<_>>()
        .join("\n\n                UNION\n\n                ");

    let sql = format!(
        "SELECT source, status_id, timestamp
             FROM (
                {branches}
             )
             ORDER BY timestamp {dir}, status_id {dir}
             LIMIT ?{limit_slot}"
    );

    HomeTimelineCandidateQuery { sql, bindings }
}

fn branch_sql(
    branch: &CandidateBranch,
    slots: &CursorSlots,
    limit_slot: usize,
    dir: &str,
) -> String {
    if branch.per_actor_limit {
        return per_actor_branch_sql(branch, slots, limit_slot, dir);
    }
    let CandidateBranch {
        source,
        from_and_where,
        timestamp_column,
        id_column,
        ..
    } = branch;
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates(timestamp_column, id_column, slots);

    format!(
        "SELECT source, status_id, timestamp
                FROM (
                    SELECT '{source}' AS source, {id_column} AS status_id, {timestamp_column} AS timestamp
                    {from_and_where}{cursor_predicates}
                    ORDER BY {timestamp_column} {dir}, {id_column} {dir}
                    LIMIT ?{limit_slot}
                )"
    )
}

/// Newest `limit` rows of one followed actor, then the branch merges them.
///
/// Cursor bounds sit inside the per-actor scan. Applying them only after the
/// per-actor `LIMIT` would drop a deep page down to whatever of those newest
/// rows still passed the cursor. `idx_remote_statuses_actor_published` covers
/// `(actor_uri, published_at, id)`, so this `ORDER BY` walks the index instead
/// of sorting each actor's history.
fn per_actor_branch_sql(
    branch: &CandidateBranch,
    slots: &CursorSlots,
    limit_slot: usize,
    dir: &str,
) -> String {
    let CandidateBranch {
        source,
        timestamp_column,
        id_column,
        ..
    } = branch;
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates("rs_actor.published_at", "rs_actor.id", slots);

    format!(
        "SELECT source, status_id, timestamp
                FROM (
                    SELECT '{source}' AS source, {id_column} AS status_id, {timestamp_column} AS timestamp
                    FROM follows f
                    JOIN remote_statuses rs
                      ON rs.id IN (
                        SELECT rs_actor.id
                        FROM remote_statuses rs_actor
                        WHERE rs_actor.actor_uri = f.target_actor_uri
                          AND rs_actor.visibility IN ('public', 'unlisted', 'private'){cursor_predicates}
                        ORDER BY rs_actor.published_at {dir}, rs_actor.id {dir}
                        LIMIT ?{limit_slot}
                      )
                    WHERE f.follower_account_id = ?1
                      AND f.state = 'accepted'
                    ORDER BY {timestamp_column} {dir}, {id_column} {dir}
                    LIMIT ?{limit_slot}
                )"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timelines::ResolvedTimelineCursor;

    fn empty_cursor() -> ResolvedTimelineCursor {
        ResolvedTimelineCursor {
            forward: false,
            max_timestamp: None,
            max_id: None,
            min_timestamp: None,
            min_id: None,
        }
    }

    fn max_cursor() -> ResolvedTimelineCursor {
        ResolvedTimelineCursor {
            forward: false,
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
            forward: false,
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
        // Bounds constrain each actor's slice. The merge orders `rs`, but it
        // does not apply the cursor a second time.
        assert!(query.sql.contains("rs_actor.published_at <= ?2"));
        assert!(query.sql.contains("rs_actor.published_at >= ?4"));
        assert!(
            query
                .sql
                .contains("(rs_actor.published_at > ?4 OR rs_actor.id > ?5)")
        );
        assert!(!query.sql.contains("rs.published_at <= ?2"));
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

    #[test]
    fn remote_followed_branch_limits_each_actor_before_the_merge() {
        let cursor = empty_cursor();
        let query = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Remote,
            false,
        );

        let actor_order = query
            .sql
            .find("ORDER BY rs_actor.published_at DESC, rs_actor.id DESC")
            .expect("per-actor order");
        let actor_limit = query.sql[actor_order..]
            .find("LIMIT ?2")
            .expect("per-actor limit");
        let merge_order = query
            .sql
            .find("ORDER BY rs.published_at DESC, rs.id DESC")
            .expect("branch merge");
        assert!(actor_order < merge_order);
        assert!(actor_order + actor_limit < merge_order);
        assert!(
            query
                .sql
                .contains("rs_actor.actor_uri = f.target_actor_uri")
        );
        assert!(
            query
                .sql
                .contains("rs_actor.visibility IN ('public', 'unlisted', 'private')")
        );
        assert!(query.sql.contains("f.state = 'accepted'"));
        // Per-actor limit, branch merge, and the outer merge share ?2.
        assert_eq!(query.sql.matches("LIMIT ?2").count(), 3);
        assert!(!query.sql.contains("UNION"));
    }

    #[test]
    fn remote_forward_page_orders_each_actor_ascending() {
        let cursor = ResolvedTimelineCursor {
            forward: true,
            max_timestamp: None,
            max_id: None,
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

        assert!(
            query
                .sql
                .contains("ORDER BY rs_actor.published_at ASC, rs_actor.id ASC")
        );
        assert!(query.sql.contains("rs_actor.published_at >= ?2"));
        assert!(
            query
                .sql
                .contains("(rs_actor.published_at > ?2 OR rs_actor.id > ?3)")
        );
        assert!(query.sql.contains("LIMIT ?4"));
    }

    #[test]
    fn remote_tag_union_and_local_branches_stay_single_ranges() {
        let cursor = empty_cursor();
        let remote = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Remote,
            true,
        );
        let local = home_timeline_candidate_query(
            "viewer",
            &cursor,
            20,
            HomeTimelineCandidateSource::Local,
            true,
        );

        assert_eq!(remote.sql.matches("UNION").count(), 1);
        assert!(remote.sql.contains("remote_status_hashtags"));
        assert!(remote.sql.contains("ON rs.id = h.status_id"));
        assert!(
            remote
                .sql
                .contains("rs_actor.actor_uri = f.target_actor_uri")
        );
        // Per-actor limit, that branch's merge, the tag branch, and the outer merge.
        assert_eq!(remote.sql.matches("LIMIT ?2").count(), 4);

        assert!(!local.sql.contains("rs_actor"));
        assert!(local.sql.contains("s.account_id = ?1"));
        assert!(local.sql.contains("s.account_id = f.target_account_id"));
    }
}
