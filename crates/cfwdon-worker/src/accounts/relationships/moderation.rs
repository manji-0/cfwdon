use super::collections::{AccountCollectionPage, AccountCollectionQuery};
use crate::{
    MastodonAccountResponse, Request, Response, Result, RouteContext,
    build_internal_cursor_link_header, find_account_by_id, find_authenticated_local_account,
    find_remote_actor_by_actor_uri, list_blocks_for_account, list_mutes_for_account, load_config,
};

pub(crate) async fn blocks_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let AccountCollectionPage {
        limit,
        max_id,
        since_id,
    } = AccountCollectionPage::from_query(&query, 20, 40)?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let blocks = list_blocks_for_account(&db, &viewer.id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for block in &blocks {
        if let Some(target_account_id) = block.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(&db, target_account_id).await?
        {
            response.push(MastodonAccountResponse::from_account(&account, &config));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(&db, &block.target_actor_uri).await? {
            response.push(MastodonAccountResponse::from_remote_actor(&actor));
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        blocks.first().map(|block| block.cursor_id),
        blocks.last().map(|block| block.cursor_id),
        blocks.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

pub(crate) async fn mutes_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let AccountCollectionPage {
        limit,
        max_id,
        since_id,
    } = AccountCollectionPage::from_query(&query, 20, 40)?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mutes = list_mutes_for_account(&db, &viewer.id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for mute in &mutes {
        if let Some(target_account_id) = mute.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(&db, target_account_id).await?
        {
            response.push(MastodonAccountResponse::from_account(&account, &config));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(&db, &mute.target_actor_uri).await? {
            response.push(MastodonAccountResponse::from_remote_actor(&actor));
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        mutes.first().map(|mute| mute.cursor_id),
        mutes.last().map(|mute| mute.cursor_id),
        mutes.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}
