use crate::{
    Request, Response, Result, RouteContext, SocialActionContextError, endorse_relationship_target,
    note_relationship_target, resolve_social_action_context,
    set_relationship_email_subscription_usecase, social_action_usecase_response,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct NoteAccountRequest {
    comment: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct EmailSubscriptionRequest {
    email_notifications: Option<bool>,
}

pub(crate) async fn pin_account_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    endorse_or_pin_account_response(req, ctx, true).await
}

pub(crate) async fn unpin_account_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    endorse_or_pin_account_response(req, ctx, false).await
}

pub(crate) async fn endorse_account_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    endorse_or_pin_account_response(req, ctx, true).await
}

pub(crate) async fn unendorse_account_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    endorse_or_pin_account_response(req, ctx, false).await
}

async fn endorse_or_pin_account_response(
    req: Request,
    ctx: RouteContext<()>,
    endorsed: bool,
) -> Result<Response> {
    let Some(context) = (match resolve_social_action_context(&req, &ctx).await {
        Ok(values) => values,
        Err(SocialActionContextError::NotFound) => {
            return Response::error("account not found", 404);
        }
        Err(SocialActionContextError::Worker(error)) => return Err(error),
    }) else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    social_action_usecase_response(
        endorse_relationship_target(
            &context.db,
            &context.config,
            context.relationship_target(),
            endorsed,
        )
        .await,
    )
}

async fn parse_note_request(req: &mut Request) -> std::result::Result<String, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let comment = if content_type.contains("application/json") {
        req.json::<NoteAccountRequest>()
            .await
            .map_err(|error| format!("invalid JSON note payload: {error}"))?
            .comment
    } else if content_type.trim().is_empty() {
        None
    } else {
        req.form_data()
            .await
            .map_err(|error| format!("invalid form note payload: {error}"))?
            .get_field("comment")
    };

    Ok(comment.unwrap_or_default().trim().to_owned())
}

pub(crate) async fn note_account_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(context) = (match resolve_social_action_context(&req, &ctx).await {
        Ok(values) => values,
        Err(SocialActionContextError::NotFound) => {
            return Response::error("account not found", 404);
        }
        Err(SocialActionContextError::Worker(error)) => return Err(error),
    }) else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let note = match parse_note_request(&mut req).await {
        Ok(note) => note,
        Err(message) => return Response::error(&message, 400),
    };

    social_action_usecase_response(
        note_relationship_target(
            &context.db,
            &context.config,
            context.relationship_target(),
            &note,
        )
        .await,
    )
}

async fn parse_email_subscription_request(req: &mut Request) -> std::result::Result<bool, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        let payload = req
            .json::<EmailSubscriptionRequest>()
            .await
            .map_err(|error| format!("invalid JSON email subscription payload: {error}"))?;
        return Ok(payload.email_notifications.unwrap_or(true));
    }

    if content_type.trim().is_empty() {
        return Ok(true);
    }

    let value = req
        .form_data()
        .await
        .map_err(|error| format!("invalid form email subscription payload: {error}"))?
        .get_field("email_notifications");
    match value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => Ok(true),
        Some("1" | "true" | "TRUE" | "on" | "ON" | "yes" | "YES") => Ok(true),
        Some("0" | "false" | "FALSE" | "off" | "OFF" | "no" | "NO") => Ok(false),
        Some(_) => Err("invalid email_notifications value".to_owned()),
    }
}

pub(crate) async fn account_email_subscriptions_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(context) = (match resolve_social_action_context(&req, &ctx).await {
        Ok(values) => values,
        Err(SocialActionContextError::NotFound) => {
            return Response::error("account not found", 404);
        }
        Err(SocialActionContextError::Worker(error)) => return Err(error),
    }) else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let enabled = match parse_email_subscription_request(&mut req).await {
        Ok(enabled) => enabled,
        Err(message) => return Response::error(&message, 400),
    };

    social_action_usecase_response(
        set_relationship_email_subscription_usecase(
            &context.db,
            context.relationship_target(),
            enabled,
        )
        .await,
    )
}
