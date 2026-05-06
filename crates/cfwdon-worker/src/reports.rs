use super::{
    D1Database, Request, Response, Result, RouteContext, build_report_response,
    find_account_by_email, find_authenticated_local_account, insert_report, list_reports,
    load_config, parse_create_report_request, resolve_account_reference, send_push_notification,
    validate_report_status_ids,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ReportRow {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) target_account_id: String,
    #[serde(rename = "target_remote_actor_uri")]
    pub(crate) _target_remote_actor_uri: Option<String>,
    pub(crate) comment: String,
    pub(crate) category: String,
    pub(crate) forward: i32,
    pub(crate) created_at: String,
}

pub(crate) async fn create_report(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let reporter = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request = match parse_create_report_request(req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    if request.account_id == reporter.id {
        return Response::error("cannot report your own account", 422);
    }

    let target = match resolve_account_reference(&db, &request.account_id).await? {
        Some(target) => target,
        None => return Response::error("account not found", 404),
    };
    let status_ids = request.status_ids.clone().unwrap_or_default();
    if let Err(message) = validate_report_status_ids(&db, &target, &status_ids).await {
        let status = if message == "status not found" {
            404
        } else {
            422
        };
        return Response::error(message, status);
    }
    let report = insert_report(&db, &reporter.id, &request, &target, &status_ids).await?;

    for admin_email in &config.admin_emails {
        if let Some(admin) = find_account_by_email(&db, admin_email).await? {
            let _ = send_push_notification(
                &db,
                &config,
                &admin.id,
                "admin.report",
                serde_json::json!({
                    "report_id": report.id,
                    "reporter_account_id": reporter.id,
                    "target_account_id": request.account_id,
                }),
            )
            .await;
        }
    }

    Response::from_json(&build_report_response(&db, &config, &report).await?)
}

pub(crate) async fn list_admin_report_notifications(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<ReportRow>> {
    list_reports(db, limit).await
}
