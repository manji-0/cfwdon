use crate::accounts::{
    FollowActionError, follow_account_usecase, parse_follow_account_request,
    unfollow_account_usecase,
};
use crate::db_session::bind_request_d1;
use crate::profile::require_authenticated_local_account;
use crate::runtime_config::load_config;
use worker::{Error, Request, Response, Result, RouteContext};

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct FollowAccountRequest {
    pub(crate) reblogs: Option<bool>,
    pub(crate) notify: Option<bool>,
    pub(crate) languages: Option<Vec<String>>,
}

pub(crate) async fn follow_account(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let request = parse_follow_account_request(&mut req).await?;

    let db = bind_request_d1(&ctx, &config)?;
    let follower = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    match follow_account_usecase(
        &db,
        &config,
        Some(&ctx.env),
        &follower,
        &target_account_id,
        &request,
    )
    .await
    {
        Ok(relationship) => Response::from_json(&relationship),
        Err(FollowActionError::NotFound) => Response::error("account not found", 404),
        Err(FollowActionError::CannotFollowSelf) => {
            Response::error("cannot follow your own account", 422)
        }
        Err(FollowActionError::Worker(error)) => Err(error),
    }
}

pub(crate) async fn unfollow_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = bind_request_d1(&ctx, &config)?;
    let follower = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    match unfollow_account_usecase(&db, &config, &follower, &target_account_id).await {
        Ok(relationship) => Response::from_json(&relationship),
        Err(FollowActionError::NotFound) => Response::error("account not found", 404),
        Err(FollowActionError::CannotFollowSelf) => {
            Response::error("cannot follow your own account", 422)
        }
        Err(FollowActionError::Worker(error)) => Err(error),
    }
}
