use super::{
    exact::dispatch_exact_without_router, fallback::run_fallback_router, fast::run_fast_router,
    selection::fast_router_kind,
};
use crate::root_document;
use crate::{CACHE_TTL_HEALTH, cache_public_response, dispatch_admin_route, is_admin_ui_path};
use worker::{Env, Request, Response, Result};

pub(crate) async fn dispatch_route(
    req: Request,
    env: Env,
    method: &str,
    path: &str,
) -> Result<Response> {
    if path.starts_with("/api/cfwdon/admin/") || is_admin_ui_path(path) {
        return dispatch_admin_route(req, env, method, path).await;
    }

    if method == "GET" && path == "/" {
        return cache_public_response(Response::from_json(&root_document())?, CACHE_TTL_HEALTH);
    }

    if method == "GET" && path == "/healthz" {
        return cache_public_response(Response::ok("ok")?, CACHE_TTL_HEALTH);
    }

    if let Some(response) = dispatch_exact_without_router(method, path, &env).await? {
        return Ok(response);
    }

    if let Some(kind) = fast_router_kind(method, path) {
        let response = run_fast_router(kind, req, env).await?;
        return Ok(response);
    }

    run_fallback_router(req, env).await
}
