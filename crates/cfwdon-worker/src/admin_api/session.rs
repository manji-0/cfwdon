use super::guard::{AdminAuthorization, authorize_admin_request};
use crate::{Response, Result, RouteContext, load_config};
use serde::Serialize;
use worker::Request;

#[derive(Debug, Serialize)]
pub(crate) struct AdminSessionResponse {
    username: String,
    email: String,
    instance_name: String,
}

pub(crate) fn is_admin_api_path(path: &str) -> bool {
    path.starts_with("/api/cfwdon/admin/")
}

pub(crate) async fn admin_me_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let account = match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(account) => account,
        AdminAuthorization::Denied(response) => return Ok(response),
    };
    let config = load_config(&ctx);

    Response::from_json(&AdminSessionResponse {
        username: account.username().to_owned(),
        email: account.access_email().to_owned(),
        instance_name: config.instance_name.clone(),
    })
}
