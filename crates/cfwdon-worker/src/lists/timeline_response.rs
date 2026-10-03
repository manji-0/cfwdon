use super::{ListTimelineQuery, list_id_from_context, list_row_by_id};
use crate::auth::find_authenticated_local_account;
use crate::db_session::{open_bound_request_session, with_d1_bookmark};
use crate::runtime_config::load_config;
use crate::timelines::list_timeline_page_response;
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn list_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: ListTimelineQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let list_id = list_id_from_context(&ctx)?;
    let (session, db) = open_bound_request_session(&ctx, &config, &req)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(list) = list_row_by_id(&db, account.id(), &list_id).await? else {
        return Response::error("list not found", 404);
    };
    let response = list_timeline_page_response(
        &req,
        &db,
        &config,
        &account,
        &list_id,
        list.replies_policy == "none",
        &pagination,
    )
    .await?;
    with_d1_bookmark(response, &session)
}
