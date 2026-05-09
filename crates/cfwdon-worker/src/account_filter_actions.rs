use super::{
    AccountReference, Request, Response, Result, RouteContext, actor_url,
    build_relationship_for_target, delete_block_by_target, delete_mute_by_target,
    expiry_from_duration_seconds, load_config, parse_mute_account_request, remote_account_rest_id,
    require_authenticated_local_account, resolve_account_reference, upsert_block, upsert_mute,
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
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if blocker.id == target.id {
                return Response::error("cannot block your own account", 422);
            }

            let target_actor_uri = actor_url(&config, &target.username);
            upsert_block(
                &db,
                &blocker.id,
                Some(target.id.as_str()),
                &target_actor_uri,
            )
            .await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &blocker,
                &target.id,
                &target_actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            upsert_block(&db, &blocker.id, None, &actor.actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &blocker,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
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
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, &target.username);
            delete_block_by_target(&db, &blocker.id, &target_actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &blocker,
                &target.id,
                &target_actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            delete_block_by_target(&db, &blocker.id, &actor.actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &blocker,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
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
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if muter.id == target.id {
                return Response::error("cannot mute your own account", 422);
            }
            let target_actor_uri = actor_url(&config, &target.username);
            upsert_mute(
                &db,
                &muter.id,
                Some(target.id.as_str()),
                &target_actor_uri,
                notifications,
                expires_at.as_deref(),
            )
            .await?;
            let relationship =
                build_relationship_for_target(&db, &config, &muter, &target.id, &target_actor_uri)
                    .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            upsert_mute(
                &db,
                &muter.id,
                None,
                &actor.actor_uri,
                notifications,
                expires_at.as_deref(),
            )
            .await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &muter,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
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
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, &target.username);
            delete_mute_by_target(&db, &muter.id, &target_actor_uri).await?;
            let relationship =
                build_relationship_for_target(&db, &config, &muter, &target.id, &target_actor_uri)
                    .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            delete_mute_by_target(&db, &muter.id, &actor.actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &muter,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}
