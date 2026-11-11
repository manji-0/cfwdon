//! Mastodon's `/api/v1/admin/ip_blocks` (`Api::V1::Admin::IpBlocksController`).

use super::guard::{AdminAuthorization, authorize_admin_request};
use super::mastodon_admin::{
    AdminPageQuery, AdminParams, admin_page_response, admin_record_not_found, admin_route_id,
    admin_validation_error,
};
use crate::db_session::bind_request_d1;
use crate::ip_blocks::{
    IpBlockExpiry, IpBlockInput, IpBlockRow, delete_ip_block, find_ip_block,
    find_ip_block_by_range, insert_ip_block, list_ip_block_page, update_ip_block,
};
use crate::request_utils::{InternalPaginationIds, empty_page_response};
use crate::runtime_config::load_config;
use crate::time_html::timestamp_to_mastodon_iso8601;
use cfwdon_domain::{IpBlockSeverity, IpRange};
use worker::{Request, Response, Result, RouteContext};

const IP_BLOCK_FIELDS: &[&str] = &["ip", "severity", "comment", "expires_in"];

fn admin_ip_block_document(row: &IpBlockRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id.to_string(),
        "ip": row.ip,
        "severity": row.severity,
        "comment": row.comment,
        "created_at": timestamp_to_mastodon_iso8601(&row.created_at),
        "expires_at": row.expires_at.as_deref().map(timestamp_to_mastodon_iso8601),
    })
}

/// How `expires_in` changes a block's expiry.
#[derive(Debug, PartialEq, Eq)]
enum ExpiryChange {
    Keep,
    Permanent,
    InSeconds(u64),
}

/// The block `params` describe, on top of `current` for an update. A create
/// needs `ip` and `severity`; `expires_in` left out keeps the current expiry,
/// and a blank or null one makes the block permanent.
fn ip_block_fields(
    params: &AdminParams,
    current: Option<&IpBlockRow>,
) -> std::result::Result<(IpRange, IpBlockSeverity, String, ExpiryChange), String> {
    let range = match params.text("ip").flatten() {
        Some(raw) => raw
            .parse::<IpRange>()
            .map_err(|_| "Validation failed: Ip is invalid".to_owned())?,
        None => current
            .and_then(|row| row.ip.parse().ok())
            .ok_or_else(|| "Validation failed: Ip can't be blank".to_owned())?,
    };
    let severity = match params.text("severity").flatten() {
        Some(name) => IpBlockSeverity::from_name(&name)
            .ok_or_else(|| "Validation failed: Severity is not included in the list".to_owned())?,
        None => current
            .and_then(|row| IpBlockSeverity::from_name(&row.severity))
            .ok_or_else(|| "Validation failed: Severity can't be blank".to_owned())?,
    };
    let comment = match params.text("comment") {
        Some(comment) => comment.unwrap_or_default(),
        None => current.map(|row| row.comment.clone()).unwrap_or_default(),
    };
    let expiry = match params.text("expires_in") {
        None => ExpiryChange::Keep,
        Some(None) => ExpiryChange::Permanent,
        Some(Some(raw)) => match raw.parse::<f64>() {
            Ok(seconds) if seconds.is_finite() && seconds >= 1.0 => {
                ExpiryChange::InSeconds(seconds as u64)
            }
            _ => return Err("Validation failed: Expires in is invalid".to_owned()),
        },
    };
    Ok((range, severity, comment, expiry))
}

fn ip_block_input(
    params: &AdminParams,
    current: Option<&IpBlockRow>,
) -> std::result::Result<IpBlockInput, String> {
    let (range, severity, comment, expiry) = ip_block_fields(params, current)?;
    let expiry = match expiry {
        ExpiryChange::Keep => {
            IpBlockExpiry::from_column(current.and_then(|row| row.expires_at.as_deref()))
        }
        ExpiryChange::Permanent => IpBlockExpiry::Permanent,
        ExpiryChange::InSeconds(seconds) => IpBlockExpiry::in_seconds(seconds)
            .map_err(|_| "Validation failed: Expires in is invalid".to_owned())?,
    };
    Ok(IpBlockInput {
        range,
        severity,
        comment,
        expiry,
    })
}

pub(crate) async fn admin_ip_blocks_list_response(
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
    let rows = list_ip_block_page(
        &db,
        limit,
        pagination.max_id,
        pagination.since_id.or(pagination.min_id),
        pagination.min_id.is_some(),
    )
    .await?;
    admin_page_response(
        &req,
        limit,
        rows.iter().map(|row| row.id),
        rows.iter().map(admin_ip_block_document).collect(),
    )
}

pub(crate) async fn admin_ip_block_show_response(
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
    match find_ip_block(&db, id).await? {
        Some(row) => Response::from_json(&admin_ip_block_document(&row)),
        None => admin_record_not_found(),
    }
}

fn ip_taken_response() -> Result<Response> {
    admin_validation_error("Validation failed: Ip has already been taken")
}

pub(crate) async fn admin_ip_block_create_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let AdminAuthorization::Denied(response) = authorize_admin_request(&req, &ctx).await? {
        return Ok(response);
    }
    let params = match AdminParams::from_request(&mut req, IP_BLOCK_FIELDS).await {
        Ok(params) => params,
        Err(message) => return admin_validation_error(&message),
    };
    let input = match ip_block_input(&params, None) {
        Ok(input) => input,
        Err(message) => return admin_validation_error(&message),
    };
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    if find_ip_block_by_range(&db, input.range).await?.is_some() {
        return ip_taken_response();
    }
    match insert_ip_block(&db, &input).await? {
        Some(row) => Response::from_json(&admin_ip_block_document(&row)),
        None => admin_record_not_found(),
    }
}

pub(crate) async fn admin_ip_block_update_response(
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
    let Some(current) = find_ip_block(&db, id).await? else {
        return admin_record_not_found();
    };
    let params = match AdminParams::from_request(&mut req, IP_BLOCK_FIELDS).await {
        Ok(params) => params,
        Err(message) => return admin_validation_error(&message),
    };
    let input = match ip_block_input(&params, Some(&current)) {
        Ok(input) => input,
        Err(message) => return admin_validation_error(&message),
    };
    if find_ip_block_by_range(&db, input.range)
        .await?
        .is_some_and(|other| other.id != id)
    {
        return ip_taken_response();
    }
    match update_ip_block(&db, id, &input).await? {
        Some(row) => Response::from_json(&admin_ip_block_document(&row)),
        None => admin_record_not_found(),
    }
}

pub(crate) async fn admin_ip_block_delete_response(
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
    if delete_ip_block(&db, id).await? {
        Response::from_json(&serde_json::json!({}))
    } else {
        admin_record_not_found()
    }
}

#[cfg(test)]
mod tests {
    use super::{AdminParams, ExpiryChange, ip_block_fields};
    use crate::ip_blocks::IpBlockRow;
    use cfwdon_domain::IpBlockSeverity;

    fn fields(
        body: serde_json::Value,
        current: Option<&IpBlockRow>,
    ) -> Result<(String, IpBlockSeverity, String, ExpiryChange), String> {
        ip_block_fields(&AdminParams::from_json(body), current).map(
            |(range, severity, comment, expiry)| (range.to_string(), severity, comment, expiry),
        )
    }

    #[test]
    fn create_reads_mastodon_fields() {
        assert_eq!(
            fields(
                serde_json::json!({
                    "ip": "203.0.113.7/24",
                    "severity": "sign_up_block",
                    "comment": " spam wave ",
                    "expires_in": 86400,
                }),
                None,
            ),
            Ok((
                "203.0.113.0/24".to_owned(),
                IpBlockSeverity::SignUpBlock,
                "spam wave".to_owned(),
                ExpiryChange::InSeconds(86_400),
            ))
        );
    }

    #[test]
    fn create_requires_ip_and_severity() {
        for (body, message) in [
            (
                serde_json::json!({ "severity": "no_access" }),
                "Validation failed: Ip can't be blank",
            ),
            (
                serde_json::json!({ "ip": "203.0.113.7" }),
                "Validation failed: Severity can't be blank",
            ),
            (
                serde_json::json!({ "ip": "example.com", "severity": "no_access" }),
                "Validation failed: Ip is invalid",
            ),
            (
                serde_json::json!({ "ip": "203.0.113.7", "severity": "silence" }),
                "Validation failed: Severity is not included in the list",
            ),
            (
                serde_json::json!({ "ip": "203.0.113.7", "severity": "no_access", "expires_in": "-5" }),
                "Validation failed: Expires in is invalid",
            ),
        ] {
            assert_eq!(fields(body, None), Err(message.to_owned()));
        }
    }

    #[test]
    fn update_keeps_unnamed_fields_and_clears_expiry_on_null() {
        let current = IpBlockRow {
            id: 4,
            ip: "198.51.100.0/24".to_owned(),
            severity: "no_access".to_owned(),
            comment: "abuse".to_owned(),
            expires_at: Some("2027-01-01T00:00:00.000Z".to_owned()),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };
        assert_eq!(
            fields(
                serde_json::json!({ "severity": "sign_up_block" }),
                Some(&current)
            ),
            Ok((
                "198.51.100.0/24".to_owned(),
                IpBlockSeverity::SignUpBlock,
                "abuse".to_owned(),
                ExpiryChange::Keep,
            ))
        );
        assert_eq!(
            fields(serde_json::json!({ "expires_in": null }), Some(&current)).map(|f| f.3),
            Ok(ExpiryChange::Permanent)
        );
    }
}
