use crate::{
    FilterActionError, Request, Response, Result, RouteContext, block_account_usecase,
    expiry_from_duration_seconds, load_config, mute_account_usecase, parse_mute_account_request,
    require_authenticated_local_account, unblock_account_usecase, unmute_account_usecase,
};
use worker::Error;

pub(crate) async fn block_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let blocker = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    match block_account_usecase(&db, &config, &blocker, &target_account_id).await {
        Ok(relationship) => Response::from_json(&relationship),
        Err(FilterActionError::NotFound) => Response::error("account not found", 404),
        Err(FilterActionError::CannotTargetSelf) => {
            Response::error("cannot block your own account", 422)
        }
        Err(FilterActionError::Worker(error)) => Err(error),
    }
}

pub(crate) async fn unblock_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let blocker = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    match unblock_account_usecase(&db, &config, &blocker, &target_account_id).await {
        Ok(relationship) => Response::from_json(&relationship),
        Err(FilterActionError::NotFound) => Response::error("account not found", 404),
        Err(FilterActionError::CannotTargetSelf) => {
            Response::error("cannot block your own account", 422)
        }
        Err(FilterActionError::Worker(error)) => Err(error),
    }
}

pub(crate) async fn mute_account(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let request = parse_mute_account_request(req)
        .await
        .map_err(Error::RustError)?;

    let db = ctx.d1(&config.database_binding)?;
    let muter = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let notifications = request.notifications.unwrap_or(true);
    let expires_at = request
        .duration
        .filter(|value| *value > 0)
        .map(expiry_from_duration_seconds)
        .transpose()?;
    match mute_account_usecase(
        &db,
        &config,
        &muter,
        &target_account_id,
        notifications,
        expires_at.as_deref(),
    )
    .await
    {
        Ok(relationship) => Response::from_json(&relationship),
        Err(FilterActionError::NotFound) => Response::error("account not found", 404),
        Err(FilterActionError::CannotTargetSelf) => {
            Response::error("cannot mute your own account", 422)
        }
        Err(FilterActionError::Worker(error)) => Err(error),
    }
}

pub(crate) async fn unmute_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let muter = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    match unmute_account_usecase(&db, &config, &muter, &target_account_id).await {
        Ok(relationship) => Response::from_json(&relationship),
        Err(FilterActionError::NotFound) => Response::error("account not found", 404),
        Err(FilterActionError::CannotTargetSelf) => {
            Response::error("cannot mute your own account", 422)
        }
        Err(FilterActionError::Worker(error)) => Err(error),
    }
}
