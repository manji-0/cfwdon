use super::guard::{AdminAuthorization, authorize_admin_request};
use super::inbox::admin_stuck_inbox_count;
use crate::db_session::bind_request_d1;
use crate::runtime_config::load_config;
use crate::tracked_d1::D1Database;
use serde::Serialize;
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Serialize)]
pub(crate) struct AdminDashboardResponse {
    pending_reports: i64,
    failed_deliveries: i64,
    queued_deliveries: i64,
    pending_background_jobs: i64,
    stuck_inbox_activities: i64,
    recent_signups: i64,
}

pub(crate) async fn admin_dashboard_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;

    let pending_reports = scalar_count(
        &db,
        "SELECT COUNT(*) AS count
         FROM reports
         WHERE action_taken = 0",
    )
    .await?;
    let failed_deliveries = scalar_count(
        &db,
        "SELECT COUNT(*) AS count
         FROM (
             SELECT id FROM outbox_deliveries WHERE state = 'failed'
             UNION ALL
             SELECT id FROM outbound_activities WHERE state = 'failed'
         )",
    )
    .await?;
    let queued_deliveries = scalar_count(
        &db,
        "SELECT COUNT(*) AS count
         FROM (
             SELECT id FROM outbox_deliveries WHERE state IN ('queued', 'expanded', 'in_flight')
             UNION ALL
             SELECT id FROM outbound_activities WHERE state IN ('queued', 'in_flight')
         )",
    )
    .await?;
    let pending_background_jobs = scalar_count(
        &db,
        "SELECT COUNT(*) AS count
         FROM background_jobs
         WHERE status IN ('pending', 'running')",
    )
    .await?;
    let stuck_inbox_activities = admin_stuck_inbox_count(&db).await?;
    let recent_signups = scalar_count(
        &db,
        "SELECT COUNT(*) AS count
         FROM accounts
         WHERE created_at >= datetime('now', '-7 days')",
    )
    .await?;

    Response::from_json(&AdminDashboardResponse {
        pending_reports,
        failed_deliveries,
        queued_deliveries,
        pending_background_jobs,
        stuck_inbox_activities,
        recent_signups,
    })
}

async fn scalar_count(db: &D1Database, sql: &str) -> Result<i64> {
    let result = db.prepare(sql).first::<serde_json::Value>(None).await?;
    Ok(result
        .and_then(|row| row.get("count").and_then(serde_json::Value::as_i64))
        .unwrap_or(0))
}
