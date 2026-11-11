//! Instance domain blocks for admins: Mastodon's `/api/v1/admin/domain_blocks`
//! and the admin UI's `/api/cfwdon/admin/domain-blocks`. Both answer with the
//! Mastodon `Admin::DomainBlock` entity.

use super::guard::{AdminAuthorization, authorize_admin_request};
use super::mastodon_admin::{
    AdminPageQuery, AdminParams, admin_page_response, admin_record_not_found, admin_route_id,
    admin_validation_error,
};
use crate::db_session::bind_request_d1;
use crate::domain_blocks::{
    InstanceDomainBlockInput, InstanceDomainBlockRow, delete_instance_domain_block,
    delete_instance_domain_block_by_id, find_instance_domain_block,
    find_instance_domain_block_by_domain, insert_instance_domain_block,
    list_instance_domain_blocks, normalize_instance_domain, update_instance_domain_block,
    upsert_instance_domain_block,
};
use crate::request_utils::{InternalPaginationIds, empty_page_response};
use crate::response_cache::sha256_hex;
use crate::runtime_config::load_config;
use crate::time_html::timestamp_to_mastodon_iso8601;
use cfwdon_domain::DomainBlockSeverity;
use worker::{Request, Response, Result, RouteContext};

const DOMAIN_BLOCK_FIELDS: &[&str] = &[
    "domain",
    "severity",
    "reject_media",
    "reject_reports",
    "private_comment",
    "public_comment",
    "obfuscate",
];

fn admin_domain_block_document(row: &InstanceDomainBlockRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id.to_string(),
        "domain": row.domain,
        "digest": sha256_hex(row.domain.as_bytes()),
        "created_at": timestamp_to_mastodon_iso8601(&row.created_at),
        "severity": row.severity,
        "reject_media": row.reject_media != 0,
        "reject_reports": row.reject_reports != 0,
        "private_comment": row.private_comment,
        "public_comment": row.public_comment,
        "obfuscate": row.obfuscate != 0,
    })
}

fn input_from_row(row: &InstanceDomainBlockRow) -> InstanceDomainBlockInput {
    InstanceDomainBlockInput {
        severity: DomainBlockSeverity::from_name(&row.severity)
            .unwrap_or(DomainBlockSeverity::Suspend),
        reject_media: row.reject_media != 0,
        reject_reports: row.reject_reports != 0,
        obfuscate: row.obfuscate != 0,
        private_comment: row.private_comment.clone(),
        public_comment: row.public_comment.clone(),
    }
}

/// The block `params` describe on top of `current` (Mastodon's defaults for a
/// new block). Fields left out keep their current value.
fn domain_block_input(
    params: &AdminParams,
    current: InstanceDomainBlockInput,
) -> std::result::Result<InstanceDomainBlockInput, String> {
    let severity = match params.text("severity").flatten() {
        Some(name) => DomainBlockSeverity::from_name(&name)
            .ok_or_else(|| "Validation failed: Severity is not included in the list".to_owned())?,
        None => current.severity,
    };
    Ok(InstanceDomainBlockInput {
        severity,
        reject_media: params.bool("reject_media")?.unwrap_or(current.reject_media),
        reject_reports: params
            .bool("reject_reports")?
            .unwrap_or(current.reject_reports),
        obfuscate: params.bool("obfuscate")?.unwrap_or(current.obfuscate),
        private_comment: params
            .text("private_comment")
            .unwrap_or(current.private_comment),
        public_comment: params
            .text("public_comment")
            .unwrap_or(current.public_comment),
    })
}

/// The domain and policy of a new block.
fn new_domain_block(
    params: &AdminParams,
) -> std::result::Result<(String, InstanceDomainBlockInput), String> {
    let domain = params
        .text("domain")
        .flatten()
        .ok_or_else(|| "Validation failed: Domain can't be blank".to_owned())?;
    let domain = normalize_instance_domain(&domain)
        .ok_or_else(|| "Validation failed: Domain is not a valid domain name".to_owned())?;
    Ok((
        domain,
        domain_block_input(params, InstanceDomainBlockInput::default())?,
    ))
}

/// `GET /api/v1/admin/domain_blocks` and the admin UI list.
pub(crate) async fn admin_domain_blocks_list_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let AdminAuthorization::Denied(response) = authorize_admin_request(&req, &ctx).await? {
        return Ok(response);
    }
    let query: AdminPageQuery = req.query().unwrap_or_default();
    let Some(pagination) = InternalPaginationIds::parse(
        query.max_id.as_deref(),
        query.since_id.as_deref(),
        query.min_id.as_deref(),
    ) else {
        return empty_page_response();
    };
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let rows = list_instance_domain_blocks(&db, limit, pagination).await?;
    admin_page_response(
        &req,
        limit,
        rows.iter().map(|row| row.id),
        rows.iter().map(admin_domain_block_document).collect(),
    )
}

pub(crate) async fn admin_domain_block_show_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let AdminAuthorization::Denied(response) = authorize_admin_request(&req, &ctx).await? {
        return Ok(response);
    }
    let Some(id) = admin_route_id(&ctx) else {
        return admin_record_not_found();
    };
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    match find_instance_domain_block(&db, id).await? {
        Some(row) => Response::from_json(&admin_domain_block_document(&row)),
        None => admin_record_not_found(),
    }
}

/// `POST /api/v1/admin/domain_blocks`: a domain that already has a block
/// answers 422 with that block, as Mastodon does.
pub(crate) async fn admin_domain_block_create_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let admin = match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(account) => account,
        AdminAuthorization::Denied(response) => return Ok(response),
    };
    let params = match AdminParams::from_request(&mut req, DOMAIN_BLOCK_FIELDS).await {
        Ok(params) => params,
        Err(message) => return admin_validation_error(&message),
    };
    let (domain, input) = match new_domain_block(&params) {
        Ok(block) => block,
        Err(message) => return admin_validation_error(&message),
    };
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    match insert_instance_domain_block(&db, &domain, &input, Some(admin.id())).await? {
        Some(row) => Response::from_json(&admin_domain_block_document(&row)),
        None => {
            let existing = find_instance_domain_block_by_domain(&db, &domain)
                .await?
                .map(|row| admin_domain_block_document(&row));
            Ok(Response::from_json(&serde_json::json!({
                "error": "Validation failed: Domain has already been taken",
                "existing_domain_block": existing,
            }))?
            .with_status(422))
        }
    }
}

/// `PUT /api/v1/admin/domain_blocks/:id`. The domain itself cannot change.
pub(crate) async fn admin_domain_block_update_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let AdminAuthorization::Denied(response) = authorize_admin_request(&req, &ctx).await? {
        return Ok(response);
    }
    let Some(id) = admin_route_id(&ctx) else {
        return admin_record_not_found();
    };
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let Some(current) = find_instance_domain_block(&db, id).await? else {
        return admin_record_not_found();
    };
    let params = match AdminParams::from_request(&mut req, DOMAIN_BLOCK_FIELDS).await {
        Ok(params) => params,
        Err(message) => return admin_validation_error(&message),
    };
    let input = match domain_block_input(&params, input_from_row(&current)) {
        Ok(input) => input,
        Err(message) => return admin_validation_error(&message),
    };
    match update_instance_domain_block(&db, id, &input).await? {
        Some(row) => Response::from_json(&admin_domain_block_document(&row)),
        None => admin_record_not_found(),
    }
}

pub(crate) async fn admin_domain_block_delete_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let AdminAuthorization::Denied(response) = authorize_admin_request(&req, &ctx).await? {
        return Ok(response);
    }
    let Some(id) = admin_route_id(&ctx) else {
        return admin_record_not_found();
    };
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    if delete_instance_domain_block_by_id(&db, id).await? {
        Response::from_json(&serde_json::json!({}))
    } else {
        admin_record_not_found()
    }
}

/// The admin UI's create: an existing block for the domain takes the new policy.
pub(crate) async fn admin_domain_blocks_create_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let admin = match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(account) => account,
        AdminAuthorization::Denied(response) => return Ok(response),
    };
    let params = match AdminParams::from_request(&mut req, DOMAIN_BLOCK_FIELDS).await {
        Ok(params) => params,
        Err(message) => return admin_validation_error(&message),
    };
    let (domain, mut input) = match new_domain_block(&params) {
        Ok(block) => block,
        Err(message) => return admin_validation_error(&message),
    };
    // The admin UI predates severities; a bare domain stays a suspension.
    if params.text("severity").flatten().is_none() {
        input.severity = DomainBlockSeverity::Suspend;
    }
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    match upsert_instance_domain_block(&db, &domain, &input, Some(admin.id())).await? {
        Some(row) => Response::from_json(&admin_domain_block_document(&row)),
        None => admin_record_not_found(),
    }
}

/// The admin UI's delete, keyed by domain.
pub(crate) async fn admin_domain_blocks_delete_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let AdminAuthorization::Denied(response) = authorize_admin_request(&req, &ctx).await? {
        return Ok(response);
    }
    let Some(domain) = ctx
        .param("id")
        .and_then(|value| normalize_instance_domain(value))
    else {
        return admin_record_not_found();
    };
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    if delete_instance_domain_block(&db, &domain).await? {
        Response::from_json(&serde_json::json!({ "deleted": true, "domain": domain }))
    } else {
        Response::error("domain block not found", 404)
    }
}

#[cfg(test)]
mod tests {
    use super::{AdminParams, InstanceDomainBlockInput, domain_block_input, new_domain_block};
    use cfwdon_domain::DomainBlockSeverity;

    #[test]
    fn new_block_defaults_to_mastodon_silence() {
        let params = AdminParams::from_json(serde_json::json!({ "domain": " Spam.Example " }));
        let (domain, input) = new_domain_block(&params).unwrap();
        assert_eq!(domain, "spam.example");
        assert_eq!(input, InstanceDomainBlockInput::default());
        assert_eq!(input.severity, DomainBlockSeverity::Silence);
    }

    #[test]
    fn new_block_reads_every_mastodon_field() {
        let params = AdminParams::from_json(serde_json::json!({
            "domain": "spam.example",
            "severity": "suspend",
            "reject_media": true,
            "reject_reports": "1",
            "obfuscate": "true",
            "private_comment": "seen in report 12",
            "public_comment": "spam",
        }));
        let (_, input) = new_domain_block(&params).unwrap();
        assert_eq!(
            input,
            InstanceDomainBlockInput {
                severity: DomainBlockSeverity::Suspend,
                reject_media: true,
                reject_reports: true,
                obfuscate: true,
                private_comment: Some("seen in report 12".to_owned()),
                public_comment: Some("spam".to_owned()),
            }
        );
    }

    #[test]
    fn invalid_fields_are_validation_errors() {
        for body in [
            serde_json::json!({}),
            serde_json::json!({ "domain": "  " }),
            serde_json::json!({ "domain": "https://spam.example/" }),
            serde_json::json!({ "domain": "spam.example", "severity": "no_access" }),
            serde_json::json!({ "domain": "spam.example", "reject_media": "maybe" }),
        ] {
            assert!(
                new_domain_block(&AdminParams::from_json(body.clone())).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn update_keeps_fields_it_does_not_name() {
        let current = InstanceDomainBlockInput {
            severity: DomainBlockSeverity::Suspend,
            reject_media: true,
            reject_reports: false,
            obfuscate: true,
            private_comment: Some("private".to_owned()),
            public_comment: Some("public".to_owned()),
        };
        let params = AdminParams::from_json(serde_json::json!({
            "severity": "silence",
            "public_comment": null,
        }));
        assert_eq!(
            domain_block_input(&params, current.clone()).unwrap(),
            InstanceDomainBlockInput {
                severity: DomainBlockSeverity::Silence,
                public_comment: None,
                ..current
            }
        );
    }
}
