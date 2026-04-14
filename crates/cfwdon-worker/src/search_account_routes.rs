use crate::account_store::{directory_order, list_discoverable_accounts, load_account_stats};
use crate::auth::extract_authenticated_user;
use crate::auth::resolve_local_account;
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use crate::{resolve_search_account, search_cached_accounts};
use serde::Deserialize;
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AccountSearchQuery {
    pub(crate) q: String,
    pub(crate) limit: Option<u32>,
    pub(crate) offset: Option<u32>,
    pub(crate) resolve: Option<bool>,
    pub(crate) following: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DirectoryQuery {
    pub(crate) limit: Option<u32>,
    pub(crate) offset: Option<u32>,
    pub(crate) local: Option<bool>,
    pub(crate) order: Option<String>,
}

pub(crate) async fn account_search(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: AccountSearchQuery = req.query().unwrap_or_default();
    let q = query.q.trim();
    if q.is_empty() {
        return Response::from_json(&Vec::<MastodonAccountResponse>::new());
    }

    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let offset = query.offset.unwrap_or(0);
    let only_following = query.following.unwrap_or(false);
    let mut results = search_cached_accounts(
        &db,
        &config,
        Some(&viewer),
        q,
        limit,
        offset,
        only_following,
    )
    .await?;

    if query.resolve.unwrap_or(false)
        && results.is_empty()
        && let Some(account) = resolve_search_account(&db, &config, q).await?
    {
        results.push(account);
    }

    Response::from_json(&results)
}

pub(crate) async fn account_directory(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: DirectoryQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let offset = query.offset.unwrap_or(0);
    let db = ctx.d1(&config.database_binding)?;

    if matches!(query.local, Some(false)) {
        return Response::from_json(&Vec::<MastodonAccountResponse>::new());
    }

    let mut response = Vec::new();
    for account in
        list_discoverable_accounts(&db, limit, offset, directory_order(query.order.as_deref()))
            .await?
    {
        let stats = load_account_stats(&db, &account.id).await?;
        response.push(MastodonAccountResponse::from_account_with_stats(
            &account, &config, &stats,
        ));
    }

    Response::from_json(&response)
}
