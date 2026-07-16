use crate::ResolvedTimelineCursor;
use worker::d1::D1Type;

pub(crate) const HOME_TIMELINE_LOCAL_CANDIDATE_SQL: &str = "SELECT source, status_id, timestamp
             FROM (
                SELECT source, status_id, timestamp
                FROM (
                    SELECT 'local' AS source, s.id AS status_id, s.created_at AS timestamp
                    FROM statuses s
                    WHERE s.account_id = ?1
                      AND (
                           ?2 IS NULL
                           OR s.created_at < ?2
                           OR (s.created_at = ?2 AND (?3 IS NULL OR s.id < ?3))
                      )
                      AND (
                           ?4 IS NULL
                           OR s.created_at > ?4
                           OR (s.created_at = ?4 AND (?5 IS NULL OR s.id > ?5))
                      )
                    ORDER BY s.created_at DESC, s.id DESC
                    LIMIT ?6
                )

                UNION

                SELECT source, status_id, timestamp
                FROM (
                    SELECT 'local' AS source, s.id AS status_id, s.created_at AS timestamp
                    FROM follows f
                    JOIN statuses s
                      ON s.account_id = f.target_account_id
                    WHERE f.follower_account_id = ?1
                      AND f.state = 'accepted'
                      AND s.visibility IN ('public', 'unlisted', 'private')
                      AND (
                           ?2 IS NULL
                           OR s.created_at < ?2
                           OR (s.created_at = ?2 AND (?3 IS NULL OR s.id < ?3))
                      )
                      AND (
                           ?4 IS NULL
                           OR s.created_at > ?4
                           OR (s.created_at = ?4 AND (?5 IS NULL OR s.id > ?5))
                      )
                    ORDER BY s.created_at DESC, s.id DESC
                    LIMIT ?6
                )
             )
             ORDER BY timestamp DESC, status_id DESC
             LIMIT ?7";

pub(crate) const HOME_TIMELINE_LOCAL_CANDIDATE_SQL_WITH_TAGS: &str =
    "SELECT source, status_id, timestamp
             FROM (
                SELECT source, status_id, timestamp
                FROM (
                    SELECT 'local' AS source, s.id AS status_id, s.created_at AS timestamp
                    FROM statuses s
                    WHERE s.account_id = ?1
                      AND (
                           ?2 IS NULL
                           OR s.created_at < ?2
                           OR (s.created_at = ?2 AND (?3 IS NULL OR s.id < ?3))
                      )
                      AND (
                           ?4 IS NULL
                           OR s.created_at > ?4
                           OR (s.created_at = ?4 AND (?5 IS NULL OR s.id > ?5))
                      )
                    ORDER BY s.created_at DESC, s.id DESC
                    LIMIT ?6
                )

                UNION

                SELECT source, status_id, timestamp
                FROM (
                    SELECT 'local' AS source, s.id AS status_id, s.created_at AS timestamp
                    FROM follows f
                    JOIN statuses s
                      ON s.account_id = f.target_account_id
                    WHERE f.follower_account_id = ?1
                      AND f.state = 'accepted'
                      AND s.visibility IN ('public', 'unlisted', 'private')
                      AND (
                           ?2 IS NULL
                           OR s.created_at < ?2
                           OR (s.created_at = ?2 AND (?3 IS NULL OR s.id < ?3))
                      )
                      AND (
                           ?4 IS NULL
                           OR s.created_at > ?4
                           OR (s.created_at = ?4 AND (?5 IS NULL OR s.id > ?5))
                      )
                    ORDER BY s.created_at DESC, s.id DESC
                    LIMIT ?6
                )

                UNION

                SELECT source, status_id, timestamp
                FROM (
                    SELECT 'local' AS source, s.id AS status_id, s.created_at AS timestamp
                    FROM followed_tags ft
                    JOIN status_hashtags h
                      ON h.tag = ft.tag_name
                    JOIN statuses s
                      ON s.id = h.status_id
                    WHERE ft.account_id = ?1
                      AND s.visibility = 'public'
                      AND (
                           ?2 IS NULL
                           OR s.created_at < ?2
                           OR (s.created_at = ?2 AND (?3 IS NULL OR s.id < ?3))
                      )
                      AND (
                           ?4 IS NULL
                           OR s.created_at > ?4
                           OR (s.created_at = ?4 AND (?5 IS NULL OR s.id > ?5))
                      )
                    ORDER BY s.created_at DESC, s.id DESC
                    LIMIT ?6
                )
             )
             ORDER BY timestamp DESC, status_id DESC
             LIMIT ?7";

pub(crate) const HOME_TIMELINE_REMOTE_CANDIDATE_SQL: &str = "SELECT source, status_id, timestamp
             FROM (
                SELECT source, status_id, timestamp
                FROM (
                    SELECT 'remote' AS source, rs.id AS status_id, rs.published_at AS timestamp
                    FROM follows f
                    JOIN remote_statuses rs
                      ON rs.actor_uri = f.target_actor_uri
                    WHERE f.follower_account_id = ?1
                      AND f.state = 'accepted'
                      AND rs.visibility IN ('public', 'unlisted', 'private')
                      AND (
                           ?2 IS NULL
                           OR rs.published_at < ?2
                           OR (rs.published_at = ?2 AND (?3 IS NULL OR rs.id < ?3))
                      )
                      AND (
                           ?4 IS NULL
                           OR rs.published_at > ?4
                           OR (rs.published_at = ?4 AND (?5 IS NULL OR rs.id > ?5))
                      )
                    ORDER BY rs.published_at DESC, rs.id DESC
                    LIMIT ?6
                )
             )
             ORDER BY timestamp DESC, status_id DESC
             LIMIT ?7";

pub(crate) const HOME_TIMELINE_REMOTE_CANDIDATE_SQL_WITH_TAGS: &str =
    "SELECT source, status_id, timestamp
             FROM (
                SELECT source, status_id, timestamp
                FROM (
                    SELECT 'remote' AS source, rs.id AS status_id, rs.published_at AS timestamp
                    FROM follows f
                    JOIN remote_statuses rs
                      ON rs.actor_uri = f.target_actor_uri
                    WHERE f.follower_account_id = ?1
                      AND f.state = 'accepted'
                      AND rs.visibility IN ('public', 'unlisted', 'private')
                      AND (
                           ?2 IS NULL
                           OR rs.published_at < ?2
                           OR (rs.published_at = ?2 AND (?3 IS NULL OR rs.id < ?3))
                      )
                      AND (
                           ?4 IS NULL
                           OR rs.published_at > ?4
                           OR (rs.published_at = ?4 AND (?5 IS NULL OR rs.id > ?5))
                      )
                    ORDER BY rs.published_at DESC, rs.id DESC
                    LIMIT ?6
                )

                UNION

                SELECT source, status_id, timestamp
                FROM (
                    SELECT 'remote' AS source, rs.id AS status_id, rs.published_at AS timestamp
                    FROM followed_tags ft
                    JOIN remote_status_hashtags h
                      ON h.tag = ft.tag_name
                    JOIN remote_statuses rs
                      ON rs.id = h.status_id
                    WHERE ft.account_id = ?1
                      AND rs.visibility = 'public'
                      AND (
                           ?2 IS NULL
                           OR rs.published_at < ?2
                           OR (rs.published_at = ?2 AND (?3 IS NULL OR rs.id < ?3))
                      )
                      AND (
                           ?4 IS NULL
                           OR rs.published_at > ?4
                           OR (rs.published_at = ?4 AND (?5 IS NULL OR rs.id > ?5))
                      )
                    ORDER BY rs.published_at DESC, rs.id DESC
                    LIMIT ?6
                )
             )
             ORDER BY timestamp DESC, status_id DESC
             LIMIT ?7";

pub(crate) fn home_timeline_local_candidate_sql(include_followed_tags: bool) -> &'static str {
    if include_followed_tags {
        HOME_TIMELINE_LOCAL_CANDIDATE_SQL_WITH_TAGS
    } else {
        HOME_TIMELINE_LOCAL_CANDIDATE_SQL
    }
}

pub(crate) fn home_timeline_remote_candidate_sql(include_followed_tags: bool) -> &'static str {
    if include_followed_tags {
        HOME_TIMELINE_REMOTE_CANDIDATE_SQL_WITH_TAGS
    } else {
        HOME_TIMELINE_REMOTE_CANDIDATE_SQL
    }
}

pub(crate) fn home_timeline_candidate_bindings<'a>(
    viewer_account_id: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
    branch_count: u32,
) -> [D1Type<'a>; 7] {
    [
        D1Type::Text(viewer_account_id),
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
        D1Type::Integer(home_timeline_candidate_query_limit(limit, branch_count) as i32),
    ]
}

pub(crate) fn home_timeline_local_branch_count(include_followed_tags: bool) -> u32 {
    if include_followed_tags { 3 } else { 2 }
}

pub(crate) fn home_timeline_remote_branch_count(include_followed_tags: bool) -> u32 {
    if include_followed_tags { 2 } else { 1 }
}

pub(crate) fn home_timeline_candidate_query_limit(limit: u32, branch_count: u32) -> u32 {
    limit.saturating_mul(branch_count.max(1))
}

pub(crate) fn home_timeline_candidate_merge_limit(limit: u32) -> u32 {
    // Preserve prior oversampling headroom used for mute/filter attrition.
    limit.saturating_mul(5)
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

    #[test]
    fn home_timeline_candidate_limits_scale_with_branch_count() {
        assert_eq!(home_timeline_candidate_query_limit(20, 2), 40);
        assert_eq!(home_timeline_candidate_query_limit(20, 3), 60);
        assert_eq!(home_timeline_candidate_merge_limit(20), 100);
        assert_eq!(home_timeline_candidate_query_limit(u32::MAX, 3), u32::MAX);
        assert_eq!(home_timeline_local_branch_count(false), 2);
        assert_eq!(home_timeline_local_branch_count(true), 3);
        assert_eq!(home_timeline_remote_branch_count(false), 1);
        assert_eq!(home_timeline_remote_branch_count(true), 2);
    }

    #[test]
    fn home_timeline_sql_variants_omit_followed_tags_when_disabled() {
        assert!(!home_timeline_local_candidate_sql(false).contains("followed_tags"));
        assert!(!home_timeline_remote_candidate_sql(false).contains("followed_tags"));
        assert!(home_timeline_local_candidate_sql(true).contains("followed_tags"));
        assert!(home_timeline_remote_candidate_sql(true).contains("followed_tags"));
    }

    #[test]
    fn home_timeline_candidate_bindings_keep_cursor_slots_stable() {
        let mut cursor = empty_cursor();
        cursor.max_timestamp = Some("2026-01-02T00:00:00Z".to_owned());
        cursor.max_id = Some("status-max".to_owned());
        cursor.min_timestamp = Some("2026-01-01T00:00:00Z".to_owned());
        cursor.min_id = Some("status-min".to_owned());

        let bindings = home_timeline_candidate_bindings("viewer", &cursor, 12, 3);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[2], D1Type::Text("status-max")));
        assert!(matches!(bindings[3], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[4], D1Type::Text("status-min")));
        assert!(matches!(bindings[5], D1Type::Integer(12)));
        assert!(matches!(bindings[6], D1Type::Integer(36)));
    }

    #[test]
    fn home_timeline_candidate_bindings_use_null_for_open_cursor_bounds() {
        let cursor = empty_cursor();
        let bindings = home_timeline_candidate_bindings("viewer", &cursor, 8, 2);

        assert!(matches!(bindings[1], D1Type::Null));
        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
    }
}
