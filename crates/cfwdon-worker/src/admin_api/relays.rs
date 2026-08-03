use super::guard::{AdminAuthorization, authorize_admin_request};
use crate::{
    Response, Result, RouteContext, create_and_enable_federation_relay, delete_federation_relay,
    disable_federation_relay, list_federation_relays,
};
use serde::Deserialize;
use worker::Request;

#[derive(Debug, Deserialize)]
struct AdminRelayRequest {
    inbox_url: Option<String>,
}

pub(crate) async fn admin_relays_list_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let relays = list_federation_relays(&db).await?;
    Response::from_json(&relays)
}

pub(crate) async fn admin_relays_create_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let admin = match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(account) => account,
        AdminAuthorization::Denied(response) => return Ok(response),
    };
    let body: AdminRelayRequest = req.json().await?;
    let Some(inbox_url) = body
        .inbox_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Response::error("inbox_url is required", 422);
    };

    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let relay = create_and_enable_federation_relay(&db, &config, &admin, inbox_url).await?;
    let _ = crate::enqueue_outbox_process_queue_if_pending(&ctx.env, &db, "relay_follow").await;
    Response::from_json(&relay)
}

pub(crate) async fn admin_relays_disable_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    };
    let relay_id = ctx
        .param("id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing relay id route parameter".to_owned()))?;

    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    if disable_federation_relay(&db, &config, relay_id).await? {
        let _ =
            crate::enqueue_outbox_process_queue_if_pending(&ctx.env, &db, "relay_unfollow").await;
        Response::from_json(&serde_json::json!({ "disabled": true, "id": relay_id }))
    } else {
        Response::error("relay not found", 404)
    }
}

pub(crate) async fn admin_relays_delete_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    };
    let relay_id = ctx
        .param("id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing relay id route parameter".to_owned()))?;

    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    if delete_federation_relay(&db, &config, relay_id).await? {
        let _ = crate::enqueue_outbox_process_queue_if_pending(&ctx.env, &db, "relay_delete").await;
        Response::from_json(&serde_json::json!({ "deleted": true, "id": relay_id }))
    } else {
        Response::error("relay not found", 404)
    }
}
