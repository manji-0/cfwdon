use crate::accounts::{
    AccountCollectionPage, AccountCollectionQuery, CursorAccountCollection,
    finalize_cursor_account_collection, list_local_endorsement_accounts,
    list_remote_endorsement_accounts,
};
use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::remote::{AccountReference, resolve_account_reference};
use crate::request_utils::empty_page_response;
use crate::runtime_config::load_config;
use worker::{Error, Request, Response, Result, RouteContext};

pub(crate) async fn endorsements_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let Some(AccountCollectionPage {
        limit,
        max_id,
        since_id,
    }) = AccountCollectionPage::from_query(&query, 40, 80)
    else {
        return empty_page_response();
    };
    let collection =
        list_local_endorsement_accounts(&db, &config, viewer.id(), limit, max_id, since_id).await?;
    endorsement_collection_response(&req, limit, max_id, since_id, collection)
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
    let db = bind_request_d1(&ctx, &config)?;
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let page = AccountCollectionPage::from_query(&query, 40, 80);
    let Some(reference) = resolve_account_reference(&db, &target_account_id).await? else {
        return Response::error("account not found", 404);
    };
    let Some(AccountCollectionPage {
        limit,
        max_id,
        since_id,
    }) = page
    else {
        return empty_page_response();
    };
    match reference {
        AccountReference::Local(account) => {
            let collection = list_local_endorsement_accounts(
                &db,
                &config,
                account.id(),
                limit,
                max_id,
                since_id,
            )
            .await?;
            endorsement_collection_response(&req, limit, max_id, since_id, collection)
        }
        AccountReference::Remote(actor) => {
            let collection = list_remote_endorsement_accounts(
                &db,
                &config,
                &actor.actor_uri,
                limit,
                max_id,
                since_id,
            )
            .await?;
            endorsement_collection_response(&req, limit, max_id, since_id, collection)
        }
    }
}

fn endorsement_collection_response(
    req: &Request,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    collection: CursorAccountCollection,
) -> Result<Response> {
    finalize_cursor_account_collection(req, limit, max_id, since_id, collection)
}
