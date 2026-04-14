use super::{
    AccountReference, Request, Response, Result, RouteContext, actor_url,
    build_relationship_for_target, delete_follow_by_target, extract_authenticated_user,
    follow_remote_account, load_config, parse_follow_account_request, resolve_account_reference,
    resolve_local_account, unfollow_remote_account, upsert_local_follow,
};
use worker::Error;

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct FollowAccountRequest {
    pub(crate) reblogs: Option<bool>,
    pub(crate) notify: Option<bool>,
    pub(crate) languages: Option<Vec<String>>,
}

pub(crate) async fn follow_account(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let request = parse_follow_account_request(&mut req).await?;

    let db = ctx.d1(&config.database_binding)?;
    let follower = resolve_local_account(&db, &user).await?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if follower.id == target.id {
                return Response::error("cannot follow your own account", 422);
            }

            upsert_local_follow(&db, &config, &follower, &target, &request).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &follower,
                &target.id,
                &actor_url(&config, &target.username),
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            let relationship =
                follow_remote_account(&db, &config, &follower, &actor, &request).await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}

pub(crate) async fn unfollow_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let follower = resolve_local_account(&db, &user).await?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, &target.username);
            delete_follow_by_target(&db, &follower.id, &target_actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &follower,
                &target.id,
                &target_actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            let relationship = unfollow_remote_account(&db, &config, &follower, &actor).await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}
