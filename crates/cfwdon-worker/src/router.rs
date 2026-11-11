use crate::app_cache::{install_app_cache, reset_app_cache_request_state};
use crate::auth::{
    apply_auth0_web_session_cookies, oauth_scope_denied, oauth_scope_denied_response,
    reset_auth0_web_session_state,
};
use crate::d1_metrics::reset_d1_request_metrics;
use crate::deferred::defer;
use crate::delivery::kick_outbox_process_queue_after_request;
use crate::federation::{install_domain_suspensions, install_remote_dns_cache};
use crate::http::install_secure_mode_fetch_signing;
use crate::ip_blocks::ip_access_denied_response;
use crate::rate_limit::{RateLimitGate, apply_rate_limit_headers, enforce_rate_limits};
use crate::response_utils::into_mutable_response;
use crate::routing::{
    HttpRequestContext, dispatch_route, ensure_missing_content_type,
    error_response_with_plain_content_type, get_request_for_head, mastodon_json_error_response,
    serves_head_as_get, should_apply_auth0_web_session_cookies, without_body,
};
use crate::runtime_config::load_config_from_env;
use crate::tracked_d1::D1Database;
use worker::{Env, Request, Response, Result, console_error};

pub(crate) async fn handle_fetch(req: Request, env: Env) -> Result<Response> {
    reset_d1_request_metrics();
    reset_app_cache_request_state();
    reset_auth0_web_session_state();
    let config = load_config_from_env(&env);
    install_remote_dns_cache(&env, &config.remote_dns_cache_binding);
    install_app_cache(&env, &config.app_cache_binding);
    install_domain_suspensions(&env, &config.database_binding);
    install_secure_mode_fetch_signing(&env, &config);

    if let Ok(db) = env.d1(&config.database_binding)
        && let Some(denied) =
            ip_access_denied_response(&req, &D1Database::new(db), &req.path()).await?
    {
        return Ok(denied);
    }

    let request_context = HttpRequestContext::from_request(&req, &env)?;
    if request_context.is_cors_preflight() {
        return request_context.cors_preflight_response();
    }

    let kick_env = env.clone();
    let method = request_context.method().to_owned();
    let path = request_context.path().to_owned();
    // After CORS preflight (a 429 still gets CORS headers from
    // `finish_response`) and before any route.
    let rate_limit = match enforce_rate_limits(&req, &env, &config, &method, &path).await? {
        RateLimitGate::Refused(response) => return request_context.finish_response(response),
        RateLimitGate::Counted(headline) => Some(headline),
        RateLimitGate::Open => None,
    };
    let head_as_get = serves_head_as_get(&method, &path);
    let (req, dispatch_method) = if head_as_get {
        (get_request_for_head(&req)?, "GET")
    } else {
        (req, method.as_str())
    };
    let response = match dispatch_route(req, env, dispatch_method, &path).await {
        Ok(response) => {
            // Durable Object / ASSETS / Cache API responses are immutable.
            let mut response = if response.status_code() == 401
                && oauth_scope_denied()
                && path.starts_with("/api/")
            {
                oauth_scope_denied_response()?
            } else {
                into_mutable_response(response)?
            };
            if let Some(headline) = &rate_limit {
                apply_rate_limit_headers(&mut response, headline)?;
            }
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

    let (response, error_message) =
        mastodon_json_error_response(request_context.path(), response).await?;
    let response = ensure_missing_content_type(response)?;
    let response = if head_as_get {
        without_body(response)?
    } else {
        response
    };
    request_context.finish_response_with_error(response, error_message.as_deref())
}
