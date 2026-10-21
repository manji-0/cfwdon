use super::{
    activitypub::{JSON_CONTENT_TYPE, static_head_response},
    exact::dispatch_exact_without_router,
    fallback::run_fallback_router,
    fast::run_fast_router,
    http::PLAIN_TEXT_CONTENT_TYPE,
    selection::fast_router_kind,
};
use crate::admin_api::dispatch_admin_route;
use crate::admin_ui::is_admin_ui_path;
use crate::response_utils::{CACHE_TTL_HEALTH, cache_public_response};
use crate::runtime_config::{load_config_from_env, root_document};
use crate::web_api::{is_web_api_path, web_session_response};
use crate::web_ui::{
    accept_prefers_web_ui_html, is_web_ui_path, web_app_url, web_ui_redirect_response,
    web_ui_response,
};
use worker::{Env, Request, Response, Result, Router};

fn is_get_or_head(method: &str) -> bool {
    matches!(method, "GET" | "HEAD")
}

// `/api/v1/accounts//followers` still matches `:id` routes with an empty
// param, which handlers report as a 500. Mastodon answers these with 404.
fn has_empty_path_segment(path: &str) -> bool {
    path.contains("//")
}

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

    // Chrome, Friendica, and SocialHub-style clients HEAD `/` (and `/healthz`).
    // GET-only registration 405s those probes even though GET `/` succeeds.
    if is_get_or_head(method) && path == "/" && accept_prefers_web_ui_html(&req)? {
        let config = load_config_from_env(&env);
        let redirect_url = web_app_url(&config, &req)?;
        return web_ui_redirect_response(redirect_url.as_str());
    }

    if is_get_or_head(method) && path == "/" {
        let response = if method == "HEAD" {
            static_head_response(JSON_CONTENT_TYPE)?
        } else {
            Response::from_json(&root_document())?
        };
        return cache_public_response(response, CACHE_TTL_HEALTH);
    }

    if is_get_or_head(method) && path == "/healthz" {
        let response = if method == "HEAD" {
            static_head_response(PLAIN_TEXT_CONTENT_TYPE)?
        } else {
            Response::ok("ok")?
        };
        return cache_public_response(response, CACHE_TTL_HEALTH);
    }

    if has_empty_path_segment(path) {
        return Response::error("Not Found", 404);
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

#[cfg(test)]
mod tests {
    use super::{has_empty_path_segment, is_get_or_head};

    #[test]
    fn origin_probes_allow_get_and_head() {
        assert!(is_get_or_head("GET"));
        assert!(is_get_or_head("HEAD"));
        assert!(!is_get_or_head("POST"));
        assert!(!is_get_or_head("OPTIONS"));
        assert!(!is_get_or_head("PUT"));
    }

    #[test]
    fn empty_path_segments_are_detected() {
        assert!(has_empty_path_segment("/api/v1/accounts//followers"));
        assert!(has_empty_path_segment("/api/v1/accounts//following"));
        assert!(has_empty_path_segment("//"));
        assert!(!has_empty_path_segment("/"));
        assert!(!has_empty_path_segment("/api/v1/accounts/1/followers"));
        assert!(!has_empty_path_segment("/api/v1/timelines/home/"));
    }
}
