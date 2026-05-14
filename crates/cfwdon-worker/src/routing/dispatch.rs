use super::{
    exact::dispatch_exact_without_router, fallback::run_fallback_router, fast::run_fast_router,
    selection::fast_router_kind,
};
use crate::root_document;
use worker::{Env, Request, Response, Result};

pub(crate) async fn dispatch_route(
    req: Request,
    env: Env,
    method: &str,
    path: &str,
) -> Result<Response> {
    if method == "GET" && path == "/" {
        return Response::from_json(&root_document());
    }

    if method == "GET" && path == "/healthz" {
        return Response::ok("ok");
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
