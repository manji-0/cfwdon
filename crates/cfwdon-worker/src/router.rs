use crate::{
    HttpRequestContext, dispatch_route, ensure_missing_content_type,
    error_response_with_plain_content_type, install_app_cache, install_remote_dns_cache,
    kick_outbox_process_queue_after_request, load_config_from_env, reset_app_cache_request_state,
    reset_d1_request_metrics,
};
use worker::{Env, Request, Response, Result, console_error};

pub(crate) async fn handle_fetch(req: Request, env: Env) -> Result<Response> {
    reset_d1_request_metrics();
    reset_app_cache_request_state();
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
        Ok(response) => response,
        Err(error) => {
            console_error!("request handler failed: {error}");
            return request_context.finish_response(error_response_with_plain_content_type(
                "Internal Server Error",
                500,
            )?);
        }
    };
    kick_outbox_process_queue_after_request(
        &kick_env,
        &config,
        &method,
        &path,
        response.status_code(),
    )
    .await;

    request_context.finish_response(ensure_missing_content_type(response)?)
}
