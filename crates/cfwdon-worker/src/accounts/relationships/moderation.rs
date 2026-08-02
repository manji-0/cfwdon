use super::collections::{
    AccountCollectionPage, AccountCollectionQuery, CursorAccountCollection,
    finalize_cursor_account_collection,
};
use crate::{
    MastodonAccountResponse, Request, Response, Result, RouteContext, find_account_by_id,
    find_authenticated_local_account, find_remote_actor_by_actor_uri, list_blocks_for_account,
    list_mutes_for_account, load_config,
};

pub(crate) async fn blocks_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let AccountCollectionPage {
        limit,
        max_id,
        since_id,
    } = AccountCollectionPage::from_query(&query, 20, 40)?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let blocks = list_blocks_for_account(&db, viewer.id(), limit, max_id, since_id).await?;
    let collection = build_moderation_account_collection(
        &db,
        &config,
        limit,
        &blocks,
        |block| block.target_account_id.as_deref(),
        |block| block.target_actor_uri.as_str(),
        |block| block.cursor_id,
    )
    .await?;

    finalize_cursor_account_collection(&req, limit, max_id, since_id, collection)
}

pub(crate) async fn mutes_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let AccountCollectionPage {
        limit,
        max_id,
        since_id,
    } = AccountCollectionPage::from_query(&query, 20, 40)?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let mutes = list_mutes_for_account(&db, viewer.id(), limit, max_id, since_id).await?;
    let collection = build_moderation_account_collection(
        &db,
        &config,
        limit,
        &mutes,
        |mute| mute.target_account_id.as_deref(),
        |mute| mute.target_actor_uri.as_str(),
        |mute| mute.cursor_id,
    )
    .await?;

    finalize_cursor_account_collection(&req, limit, max_id, since_id, collection)
}

async fn build_moderation_account_collection<T, FAccount, FActor, FCursor>(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    limit: u32,
    entries: &[T],
    target_account_id: FAccount,
    target_actor_uri: FActor,
    cursor_id: FCursor,
) -> Result<CursorAccountCollection>
where
    FAccount: Fn(&T) -> Option<&str>,
    FActor: Fn(&T) -> &str,
    FCursor: Fn(&T) -> i64,
{
    let mut accounts = Vec::new();
    for entry in entries {
        if let Some(account) = resolve_moderation_target_account_response(
            db,
            config,
            target_account_id(entry),
            target_actor_uri(entry),
        )
        .await?
        {
            accounts.push(account);
        }
    }

    Ok(CursorAccountCollection {
        first_cursor: entries.first().map(&cursor_id),
        last_cursor: entries.last().map(cursor_id),
        has_next_page: entries.len() as u32 >= limit,
        accounts,
    })
}

async fn resolve_moderation_target_account_response(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    if let Some(target_account_id) = target_account_id
        && let Some(account) = find_account_by_id(db, target_account_id).await?
    {
        return Ok(Some(MastodonAccountResponse::from_account(
            &account, config,
        )));
    }

    Ok(find_remote_actor_by_actor_uri(db, target_actor_uri)
        .await?
        .map(|actor| MastodonAccountResponse::from_remote_actor(&actor)))
}
