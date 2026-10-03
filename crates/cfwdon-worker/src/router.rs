use crate::app_cache::{install_app_cache, reset_app_cache_request_state};
use crate::auth::{apply_auth0_web_session_cookies, reset_auth0_web_session_state};
use crate::d1_metrics::reset_d1_request_metrics;
use crate::deferred::defer;
use crate::delivery::kick_outbox_process_queue_after_request;
use crate::federation::install_remote_dns_cache;
use crate::response_utils::into_mutable_response;
use crate::routing::{
    HttpRequestContext, dispatch_route, ensure_missing_content_type,
    error_response_with_plain_content_type, should_apply_auth0_web_session_cookies,
};
use crate::runtime_config::load_config_from_env;
use worker::{Env, Request, Response, Result, console_error};

pub(crate) async fn handle_fetch(req: Request, env: Env) -> Result<Response> {
    reset_d1_request_metrics();
    reset_app_cache_request_state();
    reset_auth0_web_session_state();
    let config = load_config_from_env(&env);
    install_remote_dns_cache(&env, &config.remote_dns_cache_binding);
    install_app_cache(&env, &config.app_cache_binding);

    let request_context = HttpRequestContext::from_request(&req, &env)?;
    if request_context.is_cors_preflight() {
        return request_context.cors_preflight_response();
    }

    let kick_env = env.clone();
    let method = request_context.method().to_owned();
    let path = request_context.path().to_owned();
    let response = match dispatch_route(req, env, &method, &path).await {
        Ok(response) => {
            // Durable Object / ASSETS / Cache API responses are immutable.
            let mut response = into_mutable_response(response)?;
            if should_apply_auth0_web_session_cookies(
                response.status_code(),
                response.headers().get("Upgrade")?.as_deref(),
                request_context.upgrade(),
            ) {
                apply_auth0_web_session_cookies(&mut response)?;
            }
            response
        }
        Err(error) => {
            console_error!("request handler failed: {error}");
            return request_context.finish_response(error_response_with_plain_content_type(
                "Internal Server Error",
                500,
            )?);
        }
    };
    let status_code = response.status_code();
    defer(async move {
        kick_outbox_process_queue_after_request(&kick_env, &config, &method, &path, status_code)
            .await;
    });

    request_context.finish_response(ensure_missing_content_type(response)?)
}
