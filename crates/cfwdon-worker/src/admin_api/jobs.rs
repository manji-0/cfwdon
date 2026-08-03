use super::guard::{AdminAuthorization, authorize_admin_request};
use crate::{Response, Result, RouteContext};
use serde::{Deserialize, Serialize};
use worker::{Request, d1::D1Type};

#[derive(Debug, Serialize)]
struct AdminBackgroundJobResponse {
    id: String,
    job_type: String,
    status: String,
    attempts: i32,
    next_run_at: String,
    last_error: Option<String>,
    created_at: String,
    updated_at: String,
    payload_json: String,
}

#[derive(Debug, Default, Deserialize)]
struct AdminJobsQuery {
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AdminJobRow {
    id: String,
    job_type: String,
    status: String,
    attempts: i32,
    next_run_at: String,
    last_error: Option<String>,
    created_at: String,
    updated_at: String,
    payload_json: String,
}

pub(crate) async fn admin_background_jobs_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let query: AdminJobsQuery = req.query().unwrap_or_default();
    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let status_filter = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let (sql, bindings) = if let Some(status) = status_filter {
        (
            "SELECT id, job_type, status, attempts, next_run_at, last_error, created_at, updated_at, payload_json
             FROM background_jobs
             WHERE status = ?1
             ORDER BY updated_at DESC
             LIMIT 100",
            vec![D1Type::Text(status)],
        )
    } else {
        (
            "SELECT id, job_type, status, attempts, next_run_at, last_error, created_at, updated_at, payload_json
             FROM background_jobs
             WHERE status IN ('pending', 'running', 'failed')
             ORDER BY updated_at DESC
             LIMIT 100",
            Vec::new(),
        )
    };

    let result = if bindings.is_empty() {
        db.prepare(sql).all().await?
    } else {
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    };

    let jobs = result
        .results::<AdminJobRow>()?
        .into_iter()
        .map(|row| AdminBackgroundJobResponse {
            id: row.id,
            job_type: row.job_type,
            status: row.status,
            attempts: row.attempts,
            next_run_at: row.next_run_at,
            last_error: row.last_error,
            created_at: row.created_at,
            updated_at: row.updated_at,
            payload_json: row.payload_json,
        })
        .collect::<Vec<_>>();
    Response::from_json(&jobs)
}

pub(crate) async fn admin_retry_background_job_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let job_id = ctx
        .param("id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing job id route parameter".to_owned()))?;

    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let bindings = [D1Type::Text(job_id)];
    let result = db
        .prepare(
            "UPDATE background_jobs
             SET status = 'pending',
                 next_run_at = CURRENT_TIMESTAMP,
                 last_error = NULL,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1
               AND status = 'failed'",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    if result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0 {
        Response::from_json(&serde_json::json!({ "id": job_id, "retried": true }))
    } else {
        Response::error("job not found or not in failed state", 404)
    }
}
