use super::guard::{AdminAuthorization, authorize_admin_request};
use crate::{
    ReportRow, Response, Result, RouteContext, list_report_status_ids, list_reports_filtered,
    resolve_account_reference, resolve_report, timestamp_to_mastodon_iso8601,
};
use serde::{Deserialize, Serialize};
use worker::Request;

#[derive(Debug, Default, Deserialize)]
struct AdminReportsQuery {
    status: Option<String>,
}

#[derive(Debug, Serialize)]
struct AdminReportTargetResponse {
    id: String,
    username: String,
    display_name: String,
    acct: String,
}

#[derive(Debug, Serialize)]
struct AdminReportResponse {
    id: String,
    category: String,
    comment: String,
    created_at: String,
    forwarded: bool,
    action_taken: bool,
    action_taken_at: Option<String>,
    status_ids: Vec<String>,
    target_account: AdminReportTargetResponse,
}

pub(crate) async fn admin_reports_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let query: AdminReportsQuery = req.query().unwrap_or_default();
    let pending_only = matches!(query.status.as_deref().map(str::trim), Some("pending"));
    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let reports = list_reports_filtered(&db, 100, pending_only).await?;
    let mut responses = Vec::with_capacity(reports.len());
    for report in reports {
        responses.push(build_admin_report_response(&db, &report).await?);
    }
    Response::from_json(&responses)
}

pub(crate) async fn admin_resolve_report_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let admin = match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(account) => account,
        AdminAuthorization::Denied(response) => return Ok(response),
    };
    let report_id = ctx
        .param("id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing report id route parameter".to_owned()))?
        .to_owned();
    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;

    if !resolve_report(&db, &report_id, admin.id()).await? {
        return Response::error("report not found or already resolved", 404);
    }

    let report = crate::find_report_by_id(&db, &report_id)
        .await?
        .ok_or_else(|| {
            worker::Error::RustError("resolved report could not be loaded".to_owned())
        })?;
    Response::from_json(&build_admin_report_response(&db, &report).await?)
}

async fn build_admin_report_response(
    db: &crate::D1Database,
    report: &ReportRow,
) -> Result<AdminReportResponse> {
    let target = match resolve_account_reference(db, &report.target_account_id).await? {
        Some(target) => target,
        None => {
            return Err(worker::Error::RustError(
                "reported account could not be resolved".to_owned(),
            ));
        }
    };

    let (id, username, display_name, acct) = match target {
        crate::AccountReference::Local(account) => (
            account.id().to_owned(),
            account.username().to_owned(),
            account.display_name().to_owned(),
            account.acct().to_owned(),
        ),
        crate::AccountReference::Remote(actor) => (
            crate::remote_account_rest_id(&actor.actor_uri),
            actor.username.clone(),
            actor.display_name.clone(),
            format!("{}@{}", actor.username, actor.domain),
        ),
    };

    Ok(AdminReportResponse {
        id: report.id.clone(),
        category: report.category.clone(),
        comment: report.comment.clone(),
        created_at: timestamp_to_mastodon_iso8601(&report.created_at),
        forwarded: report.forward != 0,
        action_taken: report.action_taken != 0,
        action_taken_at: report
            .action_taken_at
            .as_deref()
            .map(timestamp_to_mastodon_iso8601),
        status_ids: list_report_status_ids(db, &report.id).await?,
        target_account: AdminReportTargetResponse {
            id,
            username,
            display_name,
            acct,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::AdminReportsQuery;

    #[test]
    fn pending_status_filter_matches_pending_query_value() {
        let query = AdminReportsQuery {
            status: Some("pending".to_owned()),
        };
        assert!(matches!(
            query.status.as_deref().map(str::trim),
            Some("pending")
        ));
    }
}
