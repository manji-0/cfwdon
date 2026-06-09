use crate::{
    app_verify_credentials_response, auth0_callback_response, authorize_interaction_response,
    authorize_interaction_submit_response, create_app_response,
    oauth_authorization_server_response, oauth_authorize_response, oauth_token_response,
    oauth_userinfo_response,
};
use worker::Router;

pub(crate) fn add_oauth_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async(
            "/.well-known/oauth-authorization-server",
            |_req, ctx| async move { oauth_authorization_server_response(ctx).await },
        )
        .get_async("/api/v1/apps/verify_credentials", |req, ctx| async move {
            app_verify_credentials_response(req, ctx).await
        })
        .post_async("/api/v1/apps", |req, ctx| async move {
            create_app_response(req, ctx).await
        })
        .get_async("/oauth/userinfo", |req, ctx| async move {
            oauth_userinfo_response(req, ctx).await
        })
        .post_async("/oauth/userinfo", |req, ctx| async move {
            oauth_userinfo_response(req, ctx).await
        })
        .get_async("/oauth/authorize", |req, ctx| async move {
            oauth_authorize_response(req, ctx).await
        })
        .post_async("/oauth/authorize", |req, ctx| async move {
            oauth_authorize_response(req, ctx).await
        })
        .get_async("/oauth/auth0/callback", |req, ctx| async move {
            auth0_callback_response(req, ctx).await
        })
        .post_async("/oauth/token", |req, ctx| async move {
            oauth_token_response(req, ctx).await
        })
        .get_async("/authorize_interaction", |req, ctx| async move {
            authorize_interaction_response(req, ctx).await
        })
        .post_async("/authorize_interaction", |req, ctx| async move {
            authorize_interaction_submit_response(req, ctx).await
        })
}
