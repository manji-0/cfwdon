use crate::{
    LocalAccount, Response, Result, RouteContext, find_authenticated_local_account_with_roles,
    is_admin_authorized, load_config,
};
use worker::Request;

pub(crate) enum AdminAuthorization {
    Authorized(LocalAccount),
    Denied(Response),
}

pub(crate) async fn authorize_admin_request(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<AdminAuthorization> {
    let config = load_config(ctx);
    let db = crate::bind_request_d1(ctx, &config)?;
    Ok(
        match find_authenticated_local_account_with_roles(req, &db, &config).await? {
            Some((account, roles)) if is_admin_authorized(&config, &account, &roles) => {
                AdminAuthorization::Authorized(account)
            }
            Some(_) => AdminAuthorization::Denied(Response::error("Forbidden", 403)?),
            None => {
                AdminAuthorization::Denied(Response::error("Auth0 authentication required", 401)?)
            }
        },
    )
}
