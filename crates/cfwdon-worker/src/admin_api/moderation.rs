use super::guard::{AdminAuthorization, authorize_admin_request};
use super::reports::build_admin_report_response;
use crate::{
    AccountReference, AppConfig, D1Database, Env, Response, Result, RouteContext,
    delete_local_status_with_outbox, find_account_by_id, find_report_by_id,
    insert_instance_domain_block, list_report_status_ids, load_config, resolve_account_reference,
    resolve_report,
};
use serde::Deserialize;
use url::Url;
use worker::{Request, d1::D1Type};

#[derive(Debug, Deserialize)]
pub(crate) struct AdminReportActionRequest {
    action: String,
}

pub(crate) async fn admin_report_action_response(
    mut req: Request,
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
    let body: AdminReportActionRequest = req.json().await?;
    let action = body.action.trim();
    if action.is_empty() {
        return Response::error("action is required", 422);
    }

    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(report) = find_report_by_id(&db, &report_id).await? else {
        return Response::error("report not found", 404);
    };

    match action {
        "resolve" => apply_resolve(&db, &report, admin.id()).await?,
        "suspend_account" => apply_suspend_account(&db, &report).await?,
        "delete_statuses" => {
            apply_delete_reported_statuses(&db, &config, Some(&ctx.env), &report).await?
        }
        "block_domain" => apply_block_domain(&db, &report, &config, Some(admin.id())).await?,
        _ => return Response::error("unsupported action", 422),
    }

    if report.action_taken == 0 {
        let _ = resolve_report(&db, &report_id, admin.id()).await?;
    }

    let report = find_report_by_id(&db, &report_id)
        .await?
        .ok_or_else(|| worker::Error::RustError("report could not be reloaded".to_owned()))?;
    Response::from_json(&build_admin_report_response(&db, &report).await?)
}

async fn apply_resolve(db: &D1Database, report: &crate::ReportRow, admin_id: &str) -> Result<()> {
    if report.action_taken == 0 {
        resolve_report(db, &report.id, admin_id).await?;
    }
    Ok(())
}

async fn apply_suspend_account(db: &D1Database, report: &crate::ReportRow) -> Result<()> {
    let Some(target) = resolve_account_reference(db, &report.target_account_id).await? else {
        return Err(worker::Error::RustError(
            "reported account could not be resolved".to_owned(),
        ));
    };
    let AccountReference::Local(account) = target else {
        return Err(worker::Error::RustError(
            "suspend_account only applies to local accounts".to_owned(),
        ));
    };
    let bindings = [D1Type::Text(account.id())];
    db.prepare("UPDATE accounts SET locked = 1 WHERE id = ?1")
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    crate::invalidate_account_capabilities(account.id()).await;
    Ok(())
}

async fn apply_delete_reported_statuses(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    report: &crate::ReportRow,
) -> Result<()> {
    let status_ids = list_report_status_ids(db, &report.id).await?;
    for status_id in status_ids {
        let Some(status) = crate::find_status_by_id(db, &status_id).await? else {
            continue;
        };
        let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
            continue;
        };
        delete_local_status_with_outbox(db, config, &owner, &status).await?;
        if let Some(env) = env {
            crate::publish_user_stream_hub_event_soft(
                env,
                &config.stream_hub_binding,
                owner.id(),
                "delete",
                &status.id,
                Some(&status.id),
            )
            .await;
        }
    }
    Ok(())
}

async fn apply_block_domain(
    db: &D1Database,
    report: &crate::ReportRow,
    config: &AppConfig,
    admin_id: Option<&str>,
) -> Result<()> {
    let domain = report_domain(db, report, config).await?;
    insert_instance_domain_block(db, &domain, admin_id).await
}

async fn report_domain(
    db: &D1Database,
    report: &crate::ReportRow,
    config: &AppConfig,
) -> Result<String> {
    if let Some(actor_uri) = report._target_remote_actor_uri.as_deref() {
        return actor_uri_domain(actor_uri);
    }
    let Some(target) = resolve_account_reference(db, &report.target_account_id).await? else {
        return Err(worker::Error::RustError(
            "reported account could not be resolved".to_owned(),
        ));
    };
    match target {
        AccountReference::Remote(actor) => actor_uri_domain(&actor.actor_uri),
        AccountReference::Local(_account) => Ok(config.instance_domain.clone()),
    }
}

fn actor_uri_domain(actor_uri: &str) -> Result<String> {
    let url = Url::parse(actor_uri)
        .map_err(|error| worker::Error::RustError(format!("invalid actor uri: {error}")))?;
    url.host_str()
        .map(|host| host.to_ascii_lowercase())
        .ok_or_else(|| worker::Error::RustError("actor uri missing host".to_owned()))
}
