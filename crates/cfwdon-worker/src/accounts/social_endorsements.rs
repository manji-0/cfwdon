use crate::{
    AccountCollectionPage, AccountCollectionQuery, AccountReference, Request, Response, Result,
    RouteContext, finalize_cursor_account_collection, list_local_endorsement_accounts,
    list_remote_endorsement_accounts, load_config, require_authenticated_local_account,
    resolve_account_reference,
};
use worker::Error;

pub(crate) async fn endorsements_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let AccountCollectionPage {
        limit,
        max_id,
        since_id,
    } = AccountCollectionPage::from_query(&query, 40, 80)?;
    let collection =
        list_local_endorsement_accounts(&db, &config, &viewer.id, limit, max_id, since_id).await?;
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
    let db = ctx.d1(&config.database_binding)?;
    let query: AccountCollectionQuery = req.query().unwrap_or_default();
    let AccountCollectionPage {
        limit,
        max_id,
        since_id,
    } = AccountCollectionPage::from_query(&query, 40, 80)?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(account)) => {
            let collection =
                list_local_endorsement_accounts(&db, &config, &account.id, limit, max_id, since_id)
                    .await?;
            endorsement_collection_response(&req, limit, max_id, since_id, collection)
        }
        Some(AccountReference::Remote(actor)) => {
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
        None => Response::error("account not found", 404),
    }
}

fn endorsement_collection_response(
    req: &Request,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    collection: crate::CursorAccountCollection,
) -> Result<Response> {
    finalize_cursor_account_collection(req, limit, max_id, since_id, collection)
}
