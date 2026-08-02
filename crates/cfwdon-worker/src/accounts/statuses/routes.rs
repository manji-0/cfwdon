use super::local::local_account_statuses_response;
use super::pagination::account_statuses_request_options;
use super::remote::remote_account_statuses_response;
use super::request::{required_account_status_route_param, required_account_status_username_param};
use crate::{
    AccountReference, AccountStatusesQuery, AppConfig, LocalAccount, RemoteCollectionFetchContext,
    Request, Response, Result, RouteContext, find_account_by_username,
    find_authenticated_local_account, load_config, resolve_account_reference_with_fetch,
};

use crate::D1Database;
pub(crate) async fn account_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let account_id =
        required_account_status_route_param(ctx.param("id").map(String::as_str), "account id")?;
    account_statuses_response_for_account_id(req, ctx, account_id).await
}

pub(crate) async fn account_statuses_by_username_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let username =
        required_account_status_username_param(ctx.param("username").map(String::as_str))?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let (account, viewer) = futures_util::try_join!(
        find_account_by_username(&db, &username),
        find_authenticated_local_account(&req, &db, &config),
    )?;
    let Some(account) = account else {
        return Response::error("account not found", 404);
    };

    account_statuses_response_for_reference(
        req,
        config,
        db,
        viewer,
        Some(AccountReference::Local(account)),
    )
    .await
}

async fn account_statuses_response_for_account_id(
    req: Request,
    ctx: RouteContext<()>,
    account_id: String,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let fetch_context = RemoteCollectionFetchContext::public(&config, &db, viewer.as_ref());
    let account_ref =
        resolve_account_reference_with_fetch(&db, &account_id, Some(&fetch_context)).await?;
    account_statuses_response_for_reference(req, config, db, viewer, account_ref).await
}

async fn account_statuses_response_for_reference(
    req: Request,
    config: AppConfig,
    db: D1Database,
    viewer: Option<LocalAccount>,
    account_ref: Option<AccountReference>,
) -> Result<Response> {
    let query: AccountStatusesQuery = req.query().unwrap_or_default();
    let accept = req.headers().get("Accept")?.unwrap_or_default();
    let options = account_statuses_request_options(&query, &accept);

    match account_ref {
        Some(AccountReference::Local(account)) => {
            local_account_statuses_response(
                &req,
                &config,
                &db,
                viewer.as_ref(),
                account,
                &query,
                options.limit,
                options.query_limit,
                options.wants_html,
                options.min_id,
            )
            .await
        }
        Some(AccountReference::Remote(actor)) => {
            remote_account_statuses_response(
                &req,
                &config,
                &db,
                viewer.as_ref(),
                actor,
                &query,
                options.limit,
                options.query_limit,
                options.wants_html,
                options.min_id,
            )
            .await
        }
        None => Response::error("account not found", 404),
    }
}
