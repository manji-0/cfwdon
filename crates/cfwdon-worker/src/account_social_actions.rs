use crate::{
    AccountReference, MastodonAccountResponse, Request, Response, Result, RouteContext, actor_url,
    build_internal_cursor_link_header, build_relationship_for_target, extract_authenticated_user,
    find_account_by_id, find_follow_by_target, find_remote_actor_by_actor_uri,
    list_endorsed_accounts_for_owner, load_account_stats, load_config,
    parse_internal_pagination_id, resolve_account_reference, resolve_local_account,
    set_account_email_subscription, set_account_endorsement, set_account_note,
};
use serde::Deserialize;
use worker::Error;

#[derive(Debug, Default, Deserialize)]
struct AccountCollectionQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct NoteAccountRequest {
    comment: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct EmailSubscriptionRequest {
    email_notifications: Option<bool>,
}

async fn resolve_relationship_target(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<
    Option<(
        worker::D1Database,
        cfwdon_core::AppConfig,
        cfwdon_domain::LocalAccount,
        Option<String>,
        String,
        String,
    )>,
> {
    let config = load_config(ctx);
    let user = match extract_authenticated_user(req, &config).await? {
        Some(user) => user,
        None => return Ok(None),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let target = match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => (
            Some(target.id.clone()),
            target.id,
            actor_url(&config, &target.username),
        ),
        Some(AccountReference::Remote(actor)) => (
            None,
            crate::remote_account_rest_id(&actor.actor_uri),
            actor.actor_uri,
        ),
        None => return Err(Error::RustError("account not found".to_owned())),
    };
    Ok(Some((db, config, viewer, target.0, target.1, target.2)))
}

pub(crate) async fn endorsements_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match extract_authenticated_user(&req, &config).await? {
        Some(user) => resolve_local_account(&db, &user).await?,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    build_endorsements_response(&req, &db, &config, &viewer.id, limit, max_id, since_id).await
}

pub(crate) async fn account_endorsements_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let owner_account_id = match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(account)) => account.id,
        Some(AccountReference::Remote(_)) => {
            return Response::from_json(&Vec::<serde_json::Value>::new());
        }
        None => return Response::error("account not found", 404),
    };
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    build_endorsements_response(
        &req,
        &db,
        &config,
        &owner_account_id,
        limit,
        max_id,
        since_id,
    )
    .await
}

async fn build_endorsements_response(
    req: &Request,
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    owner_account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Response> {
    let entries =
        list_endorsed_accounts_for_owner(db, owner_account_id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for entry in &entries {
        if let Some(target_account_id) = entry.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(db, target_account_id).await?
        {
            let stats = load_account_stats(db, &account.id).await?;
            response.push(MastodonAccountResponse::from_account_with_stats(
                &account, config, &stats,
            ));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(db, &entry.target_actor_uri).await? {
            response.push(MastodonAccountResponse::from_remote_actor(&actor));
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        req,
        limit,
        entries.first().map(|entry| entry.cursor_id),
        entries.last().map(|entry| entry.cursor_id),
        entries.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
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
    let Some((db, config, viewer, target_account_id, target_id, target_actor_uri)) =
        (match resolve_relationship_target(&req, &ctx).await {
            Ok(values) => values,
            Err(Error::RustError(message)) if message == "account not found" => {
                return Response::error("account not found", 404);
            }
            Err(error) => return Err(error),
        })
    else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let Some(follow) = find_follow_by_target(&db, &viewer.id, &target_actor_uri).await? else {
        return Response::error(
            "Validation failed: You must be already following the person you want to endorse",
            422,
        );
    };
    if follow.state != "accepted" {
        return Response::error(
            "Validation failed: You must be already following the person you want to endorse",
            422,
        );
    }

    set_account_endorsement(
        &db,
        &viewer.id,
        target_account_id.as_deref(),
        &target_actor_uri,
        endorsed,
    )
    .await?;

    let relationship =
        build_relationship_for_target(&db, &config, &viewer, &target_id, &target_actor_uri).await?;
    Response::from_json(&relationship)
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
    let Some((db, config, viewer, target_account_id, target_id, target_actor_uri)) =
        (match resolve_relationship_target(&req, &ctx).await {
            Ok(values) => values,
            Err(Error::RustError(message)) if message == "account not found" => {
                return Response::error("account not found", 404);
            }
            Err(error) => return Err(error),
        })
    else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let note = match parse_note_request(&mut req).await {
        Ok(note) => note,
        Err(message) => return Response::error(&message, 400),
    };

    set_account_note(
        &db,
        &viewer.id,
        target_account_id.as_deref(),
        &target_actor_uri,
        &note,
    )
    .await?;

    let relationship =
        build_relationship_for_target(&db, &config, &viewer, &target_id, &target_actor_uri).await?;
    Response::from_json(&relationship)
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
    let Some((db, _config, viewer, target_account_id, target_id, target_actor_uri)) =
        (match resolve_relationship_target(&req, &ctx).await {
            Ok(values) => values,
            Err(Error::RustError(message)) if message == "account not found" => {
                return Response::error("account not found", 404);
            }
            Err(error) => return Err(error),
        })
    else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let enabled = match parse_email_subscription_request(&mut req).await {
        Ok(enabled) => enabled,
        Err(message) => return Response::error(&message, 400),
    };

    set_account_email_subscription(
        &db,
        &viewer.id,
        target_account_id.as_deref(),
        &target_actor_uri,
        enabled,
    )
    .await?;

    Response::from_json(&serde_json::json!({
        "id": target_id,
        "email_notifications": enabled,
    }))
}
