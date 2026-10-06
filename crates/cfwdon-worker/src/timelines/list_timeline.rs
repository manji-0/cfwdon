//! List timeline page assembly.
//!
//! Members are stored as whatever reference the client sent (local account id,
//! `user@domain`, or actor URI), so the candidate queries resolve each form in
//! SQL against the unique account and remote-actor indexes, then seek each
//! member's public statuses by time like the home timeline does.
use super::home_timeline::timeline_entries_from_candidate_rows;
use super::{
    ResolvedTimelineCursor, TimelinePaginationQuery, empty_timeline_response,
    resolve_timeline_cursor, timeline_cursor_is_unresolved, timeline_fetch_limit, timeline_limit,
    timeline_response_from_entries,
};
use crate::app_cache::load_account_capabilities;
use crate::db_utils::d1_results;
use crate::filters::{AccountFilterMatcher, load_account_filter_matcher};
use crate::home_timeline::{HomeTimelineCandidateRow, merge_home_timeline_candidate_rows};
use crate::store::relationship::list_active_muted_actor_uris_for_account;
use crate::timelines::{
    append_resolved_timeline_cursor_bindings, seekable_resolved_timeline_cursor_predicates,
};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::d1::D1Type;
use worker::{Request, Response, Result};

/// Local members: a membership row names the account id or `username@instance`.
const LOCAL_LIST_MEMBERS_SQL: &str = "SELECT a.id AS account_id
                 FROM account_list_memberships m
                 CROSS JOIN accounts a ON a.id = m.target_account_ref
                 WHERE m.list_id = ?1
                 UNION
                 SELECT a.id AS account_id
                 FROM account_list_memberships m
                 CROSS JOIN accounts a
                   ON a.username = substr(m.target_account_ref, 1, instr(m.target_account_ref, '@') - 1)
                 WHERE m.list_id = ?1
                   AND m.target_account_ref = a.username || '@' || ?2";

/// Remote members: a membership row names the actor URI or `username@domain`.
/// The handle lookup is a correlated subquery because SQLite only seeks the
/// `lower(username), lower(domain)` expression index with per-row constants.
const REMOTE_LIST_MEMBERS_SQL: &str = "SELECT ra.actor_uri AS actor_uri
                 FROM account_list_memberships m
                 CROSS JOIN remote_actors ra ON ra.actor_uri = m.target_account_ref
                 WHERE m.list_id = ?1
                 UNION
                 SELECT actor_uri FROM (
                     SELECT (
                         SELECT ra.actor_uri
                         FROM remote_actors ra
                         WHERE lower(ra.username) = lower(substr(m.target_account_ref, 1, instr(m.target_account_ref, '@') - 1))
                           AND lower(ra.domain) = lower(substr(m.target_account_ref, instr(m.target_account_ref, '@') + 1))
                     ) AS actor_uri
                     FROM account_list_memberships m
                     WHERE m.list_id = ?1
                       AND instr(m.target_account_ref, '@') > 0
                 )
                 WHERE actor_uri IS NOT NULL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListCandidateSource {
    Local,
    Remote,
}

struct ListCandidateQuery<'a> {
    sql: String,
    bindings: Vec<D1Type<'a>>,
}

fn list_candidate_query<'a>(
    list_id: &'a str,
    instance_domain: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
    exclude_replies: bool,
    source: ListCandidateSource,
) -> ListCandidateQuery<'a> {
    let dir = cursor.order_direction();
    // `?1` list id, `?2` instance domain; cursor bounds and then the limit follow.
    let mut bindings = vec![D1Type::Text(list_id), D1Type::Text(instance_domain)];
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();

    let sql = match source {
        ListCandidateSource::Local => {
            let cursor_predicates =
                seekable_resolved_timeline_cursor_predicates("s.created_at", "s.id", &slots);
            let replies = if exclude_replies {
                "\n               AND s.in_reply_to_id IS NULL"
            } else {
                ""
            };
            format!(
                "WITH members AS ({LOCAL_LIST_MEMBERS_SQL})
             SELECT 'local' AS source, s.id AS status_id, s.created_at AS timestamp
             FROM members mb
             CROSS JOIN statuses s ON s.account_id = mb.account_id
             WHERE s.visibility = 'public'{replies}{cursor_predicates}
             ORDER BY s.created_at {dir}, s.id {dir}
             LIMIT ?{limit_slot}"
            )
        }
        ListCandidateSource::Remote => {
            let cursor_predicates =
                seekable_resolved_timeline_cursor_predicates("rs.published_at", "rs.id", &slots);
            let replies = if exclude_replies {
                "\n               AND rs.in_reply_to_uri IS NULL"
            } else {
                ""
            };
            format!(
                "WITH members AS ({REMOTE_LIST_MEMBERS_SQL})
             SELECT 'remote' AS source, rs.id AS status_id, rs.published_at AS timestamp
             FROM members mb
             CROSS JOIN remote_statuses rs ON rs.actor_uri = mb.actor_uri
             WHERE rs.visibility = 'public'{replies}{cursor_predicates}
             ORDER BY rs.published_at {dir}, rs.id {dir}
             LIMIT ?{limit_slot}"
            )
        }
    };
    ListCandidateQuery { sql, bindings }
}

async fn list_candidate_rows_for_source(
    db: &D1Database,
    query: ListCandidateQuery<'_>,
) -> Result<Vec<HomeTimelineCandidateRow>> {
    let result = db
        .prepare(&query.sql)
        .bind_refs(query.bindings.iter())?
        .all()
        .await?;
    d1_results::<HomeTimelineCandidateRow>(&result)
}

/// Render one page of `list_id` for `viewer`, who owns the list.
pub(crate) async fn list_timeline_page_response(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    list_id: &str,
    exclude_replies: bool,
    pagination: &TimelinePaginationQuery,
) -> Result<Response> {
    let limit = timeline_limit(pagination);
    let query_limit = timeline_fetch_limit(limit);
    let (cursor, caps, muted_actor_uris) = futures_util::try_join!(
        resolve_timeline_cursor(db, pagination),
        load_account_capabilities(db, viewer.id()),
        list_active_muted_actor_uris_for_account(db, viewer.id()),
    )?;
    if timeline_cursor_is_unresolved(pagination, &cursor) {
        return empty_timeline_response();
    }

    let local_query = list_candidate_query(
        list_id,
        &config.instance_domain,
        &cursor,
        query_limit,
        exclude_replies,
        ListCandidateSource::Local,
    );
    let remote_query = list_candidate_query(
        list_id,
        &config.instance_domain,
        &cursor,
        query_limit,
        exclude_replies,
        ListCandidateSource::Remote,
    );
    let (filter_matcher, local_rows, remote_rows) = futures_util::try_join!(
        async {
            if caps.has_filters {
                load_account_filter_matcher(db, viewer.id()).await
            } else {
                Ok(AccountFilterMatcher::default())
            }
        },
        list_candidate_rows_for_source(db, local_query),
        list_candidate_rows_for_source(db, remote_query),
    )?;
    let candidate_rows =
        merge_home_timeline_candidate_rows(local_rows, remote_rows, query_limit, cursor.forward);

    let entries = timeline_entries_from_candidate_rows(
        db,
        config,
        viewer,
        &filter_matcher,
        caps.has_thread_mutes,
        &muted_actor_uris,
        candidate_rows,
        limit,
        cursor.forward,
    )
    .await?;
    timeline_response_from_entries(req, limit, cursor.forward, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cursor() -> ResolvedTimelineCursor {
        ResolvedTimelineCursor {
            forward: false,
            max_timestamp: None,
            max_id: None,
            min_timestamp: None,
            min_id: None,
        }
    }

    #[test]
    fn local_list_query_binds_list_domain_and_limit() {
        let cursor = empty_cursor();
        let query = list_candidate_query(
            "list-1",
            "social.example",
            &cursor,
            80,
            false,
            ListCandidateSource::Local,
        );
        assert_eq!(query.bindings.len(), 3);
        assert!(matches!(query.bindings[0], D1Type::Text("list-1")));
        assert!(matches!(query.bindings[1], D1Type::Text("social.example")));
        assert!(matches!(query.bindings[2], D1Type::Integer(80)));
        assert!(query.sql.contains("LIMIT ?3"));
        assert!(!query.sql.contains("in_reply_to_id IS NULL"));
    }

    #[test]
    fn remote_list_query_excludes_replies_and_seeks_cursor() {
        let cursor = ResolvedTimelineCursor {
            forward: false,
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: None,
            min_id: None,
        };
        let query = list_candidate_query(
            "list-1",
            "social.example",
            &cursor,
            80,
            true,
            ListCandidateSource::Remote,
        );
        assert_eq!(query.bindings.len(), 5);
        assert!(query.sql.contains("rs.in_reply_to_uri IS NULL"));
        assert!(query.sql.contains("rs.published_at <= ?3"));
        assert!(query.sql.contains("LIMIT ?5"));
    }
}
