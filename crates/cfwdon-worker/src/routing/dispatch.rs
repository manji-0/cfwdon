use super::{
    exact::dispatch_exact_without_router, fallback::run_fallback_router, fast::run_fast_router,
    selection::fast_router_kind,
};
use crate::root_document;
use crate::{
    CACHE_TTL_HEALTH, accept_prefers_web_ui_html, cache_public_response, dispatch_admin_route,
    is_admin_ui_path, is_web_api_path, is_web_ui_path, load_config_from_env, web_app_url,
    web_session_response, web_ui_redirect_response, web_ui_response,
};
use worker::{Env, Request, Response, Result, Router};

pub(crate) async fn dispatch_route(
    req: Request,
    env: Env,
    method: &str,
    path: &str,
) -> Result<Response> {
    if path.starts_with("/api/cfwdon/admin/") || is_admin_ui_path(path) {
        return dispatch_admin_route(req, env, method, path).await;
    }

    if is_web_api_path(path) {
        return web_api_router().run(req, env).await;
    }

    if is_web_ui_path(path) {
        return web_ui_router().run(req, env).await;
    }

    if method == "GET" && path == "/" && accept_prefers_web_ui_html(&req)? {
        let config = load_config_from_env(&env);
        let redirect_url = web_app_url(&config, &req)?;
        return web_ui_redirect_response(redirect_url.as_str());
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

fn web_api_router() -> Router<'static, ()> {
    Router::new().get_async("/api/cfwdon/web/session", |req, ctx| async move {
        web_session_response(req, ctx).await
    })
}

fn web_ui_router() -> Router<'static, ()> {
    Router::new()
        .get_async(
            "/app",
            |req, ctx| async move { web_ui_response(req, ctx).await },
        )
        .get_async("/app/", |req, ctx| async move {
            web_ui_response(req, ctx).await
        })
        .get_async("/app/login", |req, ctx| async move {
            web_ui_response(req, ctx).await
        })
        .get_async("/app/logout", |req, ctx| async move {
            web_ui_response(req, ctx).await
        })
        .get_async("/app/*rest", |req, ctx| async move {
            web_ui_response(req, ctx).await
        })
}
