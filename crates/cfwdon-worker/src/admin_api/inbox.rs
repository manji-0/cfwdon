use super::guard::{AdminAuthorization, authorize_admin_request};
use crate::{Response, Result, RouteContext, reclaim_stale_inbox_activities};
use serde::{Deserialize, Serialize};
use worker::Request;

#[derive(Debug, Serialize)]
struct AdminInboxActivityResponse {
    actor_uri: String,
    activity_id: String,
    activity_type: String,
    created_at: String,
    processed_at: Option<String>,
    completion_state: String,
}

#[derive(Debug, Default, Deserialize)]
struct AdminInboxQuery {
    pending: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AdminInboxRow {
    actor_uri: String,
    activity_id: String,
    activity_type: String,
    created_at: String,
    processed_at: Option<String>,
    completion_state: String,
}

const INBOX_LIST_SQL: &str = "SELECT
    ia.actor_uri,
    ia.activity_id,
    ia.activity_type,
    ia.created_at,
    ia.processed_at,
    CASE
        WHEN ia.processed_at IS NOT NULL THEN 'completed'
        WHEN ia.activity_type = 'Create' AND EXISTS (
            SELECT 1 FROM remote_statuses rs
            WHERE rs.object_uri = REPLACE(ia.activity_id, '/activity', '')
        ) THEN 'effect_applied'
        WHEN ia.activity_type = 'Update' AND (
            (ia.activity_id LIKE '%#updates/%' AND EXISTS (
                SELECT 1 FROM remote_actors ra WHERE ra.actor_uri = ia.actor_uri
            ))
            OR EXISTS (
                SELECT 1 FROM remote_statuses rs
                WHERE rs.object_uri = REPLACE(ia.activity_id, '/activity', '')
                   OR rs.object_uri = ia.activity_id
            )
        ) THEN 'effect_applied'
        WHEN ia.activity_type = 'Delete' AND NOT EXISTS (
            SELECT 1 FROM remote_statuses rs
            WHERE rs.object_uri = ia.activity_id OR rs.url = ia.activity_id
        ) THEN 'effect_applied'
        WHEN ia.created_at > datetime('now', '-15 minutes') THEN 'in_flight'
        ELSE 'stuck'
    END AS completion_state
 FROM inbox_activities ia";

pub(crate) async fn admin_inbox_activities_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let query: AdminInboxQuery = req.query().unwrap_or_default();
    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;

    let sql = if query.pending.unwrap_or(false) {
        format!(
            "{INBOX_LIST_SQL}
             WHERE ia.processed_at IS NULL
             ORDER BY ia.created_at DESC
             LIMIT 100"
        )
    } else {
        format!(
            "{INBOX_LIST_SQL}
             ORDER BY ia.created_at DESC
             LIMIT 100"
        )
    };

    let result = db.prepare(&sql).all().await?;
    let activities = result
        .results::<AdminInboxRow>()?
        .into_iter()
        .map(|row| AdminInboxActivityResponse {
            actor_uri: row.actor_uri,
            activity_id: row.activity_id,
            activity_type: row.activity_type,
            created_at: row.created_at,
            processed_at: row.processed_at,
            completion_state: row.completion_state,
        })
        .collect::<Vec<_>>();
    Response::from_json(&activities)
}

#[derive(Debug, Serialize)]
struct AdminInboxReclaimResponse {
    marked_processed: u32,
    released: u32,
}

pub(crate) async fn admin_reclaim_inbox_activities_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let report = reclaim_stale_inbox_activities(&db, 100).await?;
    Response::from_json(&AdminInboxReclaimResponse {
        marked_processed: report.marked_processed,
        released: report.released,
    })
}

pub(crate) async fn admin_stuck_inbox_count(db: &crate::D1Database) -> Result<i64> {
    let result = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM inbox_activities ia
             WHERE ia.processed_at IS NULL
               AND ia.created_at <= datetime('now', '-15 minutes')
               AND NOT (
                 (ia.activity_type = 'Create' AND EXISTS (
                   SELECT 1 FROM remote_statuses rs
                   WHERE rs.object_uri = REPLACE(ia.activity_id, '/activity', '')
                 ))
                 OR (ia.activity_type = 'Update' AND (
                   (ia.activity_id LIKE '%#updates/%' AND EXISTS (
                     SELECT 1 FROM remote_actors ra WHERE ra.actor_uri = ia.actor_uri
                   ))
                   OR EXISTS (
                     SELECT 1 FROM remote_statuses rs
                     WHERE rs.object_uri = REPLACE(ia.activity_id, '/activity', '')
                        OR rs.object_uri = ia.activity_id
                   )
                 ))
                 OR (ia.activity_type = 'Delete' AND NOT EXISTS (
                   SELECT 1 FROM remote_statuses rs
                   WHERE rs.object_uri = ia.activity_id OR rs.url = ia.activity_id
                 ))
               )",
        )
        .first::<serde_json::Value>(None)
        .await?;
    Ok(result
        .and_then(|row| row.get("count").and_then(serde_json::Value::as_i64))
        .unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use crate::InboxReclaimReport;

    #[test]
    fn inbox_reclaim_report_defaults_to_zero() {
        let report = InboxReclaimReport::default();
        assert_eq!(report.marked_processed, 0);
        assert_eq!(report.released, 0);
    }
}
