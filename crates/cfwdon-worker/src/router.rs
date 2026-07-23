use crate::{HttpRequestContext, dispatch_route, install_remote_dns_cache, load_config_from_env};
use worker::{Env, Request, Response, Result};

pub(crate) async fn handle_fetch(req: Request, env: Env) -> Result<Response> {
    let config = load_config_from_env(&env);
    install_remote_dns_cache(&env, &config.remote_dns_cache_binding);

    let request_context = HttpRequestContext::from_request(&req, &env)?;
    if request_context.is_cors_preflight() {
        return request_context.cors_preflight_response();
    }

    let response =
        dispatch_route(req, env, request_context.method(), request_context.path()).await?;

    request_context.finish_response(response)
}
