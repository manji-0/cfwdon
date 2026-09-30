use super::guard::{AdminAuthorization, authorize_admin_request};
use crate::db_session::bind_request_d1;
use crate::domain_blocks::{
    InstanceDomainBlockRow, delete_instance_domain_block, insert_instance_domain_block,
    list_instance_domain_blocks,
};
use crate::runtime_config::load_config;
use crate::time_html::now_iso_string;
use serde::Deserialize;
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Deserialize)]
struct AdminDomainBlockRequest {
    domain: Option<String>,
}

pub(crate) async fn admin_domain_blocks_list_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let blocks = list_instance_domain_blocks(&db, 200).await?;
    Response::from_json(&blocks)
}

pub(crate) async fn admin_domain_blocks_create_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let admin = match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(account) => account,
        AdminAuthorization::Denied(response) => return Ok(response),
    };
    let body: AdminDomainBlockRequest = req.json().await?;
    let Some(domain) = body
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Response::error("domain is required", 422);
    };

    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    insert_instance_domain_block(&db, domain, Some(admin.id())).await?;
    let blocks = list_instance_domain_blocks(&db, 200).await?;
    if let Some(created) = blocks
        .iter()
        .find(|row| row.domain == domain.to_ascii_lowercase())
    {
        return Response::from_json(created);
    }
    Response::from_json(&InstanceDomainBlockRow {
        id: 0,
        domain: domain.to_ascii_lowercase(),
        created_at: now_iso_string()?,
    })
}

pub(crate) async fn admin_domain_blocks_delete_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }
    let domain = ctx
        .param("id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing domain route parameter".to_owned()))?;

    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    if delete_instance_domain_block(&db, domain).await? {
        Response::from_json(&serde_json::json!({ "deleted": true, "domain": domain }))
    } else {
        Response::error("domain block not found", 404)
    }
}
