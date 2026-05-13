use crate::{D1Database, ResolvedTimelineCursor, Result};
use serde::Deserialize;
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
) -> Result<Vec<HomeTimelineCandidateRow>> {
    let outer_limit = limit.saturating_mul(5);
    let bindings = [
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
        D1Type::Integer(outer_limit as i32),
    ];
    let result = db
        .prepare(
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

                UNION

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
             LIMIT ?7",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<HomeTimelineCandidateRow>()
}
