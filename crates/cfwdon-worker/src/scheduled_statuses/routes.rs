use super::{
    build_scheduled_status_document, delete_scheduled_status, find_scheduled_status_for_account,
    insert_scheduled_status, list_scheduled_statuses_for_account, update_scheduled_status_time,
};
use crate::auth::LocalApiAuthentication;
use crate::{
    AppConfig, D1Database, Request, Response, Result, RouteContext, StatusDraft,
    app_bearer_token_from_request, authenticate_local_api_request,
    build_internal_cursor_link_for_url_with_min_id, find_oauth_app_id_by_bearer_token, load_config,
    normalize_scheduled_at, oauth_access_token_has_any_scope, parse_internal_pagination_id,
    require_authenticated_local_account, validate_scheduled_at_minimum_offset,
};
use serde::Deserialize;
use worker::Error;

async fn require_scheduled_status_id(ctx: &RouteContext<()>) -> Result<String> {
    ctx.param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing scheduled status id".to_owned()))
}

async fn require_authenticated_scheduled_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<crate::LocalAccount>> {
    require_authenticated_local_account(req, db, config).await
}

#[derive(Debug)]
struct ScheduledStatusRequestAccess {
    viewer: crate::LocalAccount,
    application_id: Option<i64>,
}

async fn request_scheduled_status_application_id(
    req: &Request,
    db: &D1Database,
) -> Result<Option<i64>> {
    let Some(token) = app_bearer_token_from_request(req)? else {
        return Ok(None);
    };
    let app_id = find_oauth_app_id_by_bearer_token(db, &token)
        .await?
        .ok_or_else(|| Error::RustError("invalid scheduled status app bearer token".to_owned()))?;
    Ok(Some(app_id))
}

fn scheduled_statuses_unauthorized_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "The access token is invalid",
    }))?
    .with_status(401))
}

fn scheduled_statuses_not_found_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "Record not found",
    }))?
    .with_status(404))
}

fn scheduled_statuses_outside_authorized_scopes_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is outside the authorized scopes",
    }))?
    .with_status(403))
}

#[derive(Debug, Default, Deserialize)]
struct ScheduledStatusesQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateScheduledStatusRequest {
    scheduled_at: Option<String>,
}

async fn parse_scheduled_status_update_request(
    req: &mut Request,
) -> std::result::Result<UpdateScheduledStatusRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        req.json::<UpdateScheduledStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON scheduled status payload: {error}"))
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form scheduled status payload: {error}"))?;
        Ok(UpdateScheduledStatusRequest {
            scheduled_at: form.get_field("scheduled_at"),
        })
    }
}

async fn resolve_scheduled_status_request_access(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
    scopes: &[&str],
) -> Result<Option<ScheduledStatusRequestAccess>> {
    match authenticate_local_api_request(req, db, config).await? {
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, scopes) {
                return Err(Error::RustError(
                    "scheduled status token outside authorized scopes".to_owned(),
                ));
            }
            Ok(Some(ScheduledStatusRequestAccess {
                viewer: auth.account,
                application_id: Some(auth.token.oauth_app_id),
            }))
        }
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => Ok(None),
        LocalApiAuthentication::Auth0(viewer) => {
            let application_id = match request_scheduled_status_application_id(req, db).await {
                Ok(value) => value,
                Err(Error::RustError(message))
                    if message == "invalid scheduled status app bearer token" =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            Ok(Some(ScheduledStatusRequestAccess {
                viewer,
                application_id,
            }))
        }
        LocalApiAuthentication::None => {
            Ok(require_authenticated_scheduled_account(req, db, config)
                .await?
                .map(|viewer| ScheduledStatusRequestAccess {
                    viewer,
                    application_id: None,
                }))
        }
    }
}

fn build_scheduled_statuses_link_header(
    req: &Request,
    limit: u32,
    first_cursor: Option<i64>,
    last_cursor: Option<i64>,
    has_next: bool,
) -> Result<Option<String>> {
    let url = req.url()?;
    let mut links = Vec::new();

    if has_next && let Some(cursor) = last_cursor {
        links.push(build_internal_cursor_link_for_url_with_min_id(
            &url,
            limit,
            Some(cursor),
            None,
            None,
            "next",
        )?);
    }

    if let Some(cursor) = first_cursor {
        links.push(build_internal_cursor_link_for_url_with_min_id(
            &url,
            limit,
            None,
            None,
            Some(cursor),
            "prev",
        )?);
    }

    if links.is_empty() {
        return Ok(None);
    }

    Ok(Some(links.join(", ")))
}

pub(crate) async fn scheduled_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let access = match resolve_scheduled_status_request_access(
        &req,
        &db,
        &config,
        &["read:statuses", "read"],
    )
    .await
    {
        Ok(Some(access)) => access,
        Ok(None) => return scheduled_statuses_unauthorized_response(),
        Err(Error::RustError(message))
            if message == "scheduled status token outside authorized scopes" =>
        {
            return scheduled_statuses_outside_authorized_scopes_response();
        }
        Err(error) => return Err(error),
    };
    let query: ScheduledStatusesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let mut statuses = list_scheduled_statuses_for_account(
        &db,
        access.viewer.id(),
        access.application_id,
        limit.saturating_add(1),
        max_id,
        since_id,
        min_id,
    )
    .await?;
    let has_next = statuses.len() as u32 > limit;
    if has_next {
        statuses.truncate(limit as usize);
    }
    let mut documents = Vec::new();
    for status in &statuses {
        documents.push(build_scheduled_status_document(&db, &config, status).await?);
    }
    let mut builder = Response::builder();
    if let Some(link_header) = build_scheduled_statuses_link_header(
        &req,
        limit,
        statuses.first().map(|status| status.cursor_id),
        statuses.last().map(|status| status.cursor_id),
        has_next,
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }
    builder.from_json(&documents)
}

pub(crate) async fn scheduled_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let access = match resolve_scheduled_status_request_access(
        &req,
        &db,
        &config,
        &["read:statuses", "read"],
    )
    .await
    {
        Ok(Some(access)) => access,
        Ok(None) => return scheduled_statuses_unauthorized_response(),
        Err(Error::RustError(message))
            if message == "scheduled status token outside authorized scopes" =>
        {
            return scheduled_statuses_outside_authorized_scopes_response();
        }
        Err(error) => return Err(error),
    };
    let id = require_scheduled_status_id(&ctx).await?;
    let Some(status) =
        find_scheduled_status_for_account(&db, access.viewer.id(), access.application_id, &id)
            .await?
    else {
        return scheduled_statuses_not_found_response();
    };
    Response::from_json(&build_scheduled_status_document(&db, &config, &status).await?)
}

pub(crate) async fn update_scheduled_status_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let access = match resolve_scheduled_status_request_access(
        &req,
        &db,
        &config,
        &["write:statuses", "write"],
    )
    .await
    {
        Ok(Some(access)) => access,
        Ok(None) => return scheduled_statuses_unauthorized_response(),
        Err(Error::RustError(message))
            if message == "scheduled status token outside authorized scopes" =>
        {
            return scheduled_statuses_outside_authorized_scopes_response();
        }
        Err(error) => return Err(error),
    };
    let id = require_scheduled_status_id(&ctx).await?;
    let Some(current) =
        find_scheduled_status_for_account(&db, access.viewer.id(), access.application_id, &id)
            .await?
    else {
        return scheduled_statuses_not_found_response();
    };
    let request = match parse_scheduled_status_update_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let scheduled_at = match normalize_scheduled_at(request.scheduled_at.as_deref()) {
        Ok(Some(value)) => value,
        Ok(None) => current.scheduled_at.clone(),
        Err(message) => return Response::error(message, 422),
    };
    if let Err(message) = validate_scheduled_at_minimum_offset(&scheduled_at) {
        return Response::error(message, 422);
    }
    update_scheduled_status_time(
        &db,
        access.viewer.id(),
        access.application_id,
        &id,
        &scheduled_at,
    )
    .await?;
    let Some(updated) =
        find_scheduled_status_for_account(&db, access.viewer.id(), access.application_id, &id)
            .await?
    else {
        return scheduled_statuses_not_found_response();
    };
    Response::from_json(&build_scheduled_status_document(&db, &config, &updated).await?)
}

pub(crate) async fn create_scheduled_status(
    db: &D1Database,
    config: &AppConfig,
    account_id: &str,
    draft: &StatusDraft,
    idempotency_key: Option<&str>,
    application_id: Option<i64>,
    quote_of_uri: Option<&str>,
    scheduled_at: &str,
) -> Result<serde_json::Value> {
    let status = insert_scheduled_status(
        db,
        account_id,
        draft,
        idempotency_key,
        application_id,
        quote_of_uri,
        scheduled_at,
    )
    .await?;
    build_scheduled_status_document(db, config, &status).await
}

pub(crate) async fn delete_scheduled_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let access = match resolve_scheduled_status_request_access(
        &req,
        &db,
        &config,
        &["write:statuses", "write"],
    )
    .await
    {
        Ok(Some(access)) => access,
        Ok(None) => return scheduled_statuses_unauthorized_response(),
        Err(Error::RustError(message))
            if message == "scheduled status token outside authorized scopes" =>
        {
            return scheduled_statuses_outside_authorized_scopes_response();
        }
        Err(error) => return Err(error),
    };
    let id = require_scheduled_status_id(&ctx).await?;
    if !delete_scheduled_status(&db, access.viewer.id(), access.application_id, &id).await? {
        return scheduled_statuses_not_found_response();
    }
    Response::from_json(&serde_json::json!({}))
}
