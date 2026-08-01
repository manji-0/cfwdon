use crate::{
    Response, Result, RouteContext, admin_create_custom_emoji_response,
    admin_custom_emojis_response, admin_delete_custom_emoji_response,
    admin_update_custom_emoji_response,
};
use worker::Request;

pub(crate) async fn admin_emojis_list_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    admin_custom_emojis_response(req, ctx).await
}

pub(crate) async fn admin_emojis_create_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    admin_create_custom_emoji_response(req, ctx).await
}

pub(crate) async fn admin_emojis_update_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    admin_update_custom_emoji_response(req, ctx).await
}

pub(crate) async fn admin_emojis_delete_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    admin_delete_custom_emoji_response(req, ctx).await
}
