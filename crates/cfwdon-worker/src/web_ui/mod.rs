mod assets {
    include!(concat!(env!("OUT_DIR"), "/web_ui_assets.rs"));
}

use crate::{
    auth0_login_redirect_response, auth0_logout_redirect_response, instance_base_url, load_config,
};
use assets::lookup_web_embedded_asset;
use url::Url;
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn web_ui_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let path = req.path();

    if path == "/app/login" {
        return web_login_redirect(&config, &req);
    }
    if path == "/app/logout" {
        return auth0_logout_redirect_response(&config);
    }
    if is_public_web_asset_path(&path) {
        return serve_embedded_asset(&path);
    }
    if is_web_ui_path(&path) {
        return serve_embedded_asset("/app/");
    }

    Response::error("Not Found", 404)
}

pub(crate) fn is_web_ui_path(path: &str) -> bool {
    path == "/app" || path.starts_with("/app/")
}

pub(crate) fn is_public_web_asset_path(path: &str) -> bool {
    path.starts_with("/app/assets/")
}

pub(crate) fn accept_header_prefers_web_ui_html(accept: &str) -> bool {
    let accept = accept.to_ascii_lowercase();
    accept.contains("text/html")
        && !accept.contains("application/json")
        && !accept.contains("application/activity+json")
}

pub(crate) fn accept_prefers_web_ui_html(req: &Request) -> Result<bool> {
    let accept = req.headers().get("Accept")?.unwrap_or_default();
    Ok(accept_header_prefers_web_ui_html(&accept))
}

fn serve_embedded_asset(path: &str) -> Result<Response> {
    let (bytes, content_type) = lookup_web_embedded_asset(path)
        .ok_or_else(|| worker::Error::RustError(format!("web ui asset not found: {path}")))?;
    let mut response = Response::from_bytes(bytes.to_vec())?;
    response.headers_mut().set("Content-Type", content_type)?;
    if path.ends_with(".html") || path == "/app/" {
        response.headers_mut().set("Cache-Control", "no-cache")?;
    } else {
        response
            .headers_mut()
            .set("Cache-Control", "public, max-age=3600")?;
    }
    Ok(response)
}

fn web_login_redirect(config: &crate::AppConfig, req: &Request) -> Result<Response> {
    let return_url = web_return_url(config, req)?;
    auth0_login_redirect_response(config, &return_url, &return_url)
}

fn web_return_url(config: &crate::AppConfig, req: &Request) -> Result<Url> {
    let mut return_url = req.url()?;
    return_url.set_path("/app/");
    return_url.set_query(None);
    if return_url.host_str().is_none() {
        return_url =
            Url::parse(&format!("{}/app/", instance_base_url(config))).map_err(|error| {
                worker::Error::RustError(format!("invalid web ui return URL: {error}"))
            })?;
    }
    Ok(return_url)
}

#[cfg(test)]
mod tests {
    use super::{accept_header_prefers_web_ui_html, is_public_web_asset_path, is_web_ui_path};

    #[test]
    fn web_ui_paths_are_detected() {
        assert!(is_web_ui_path("/app"));
        assert!(is_web_ui_path("/app/"));
        assert!(is_web_ui_path("/app/notifications"));
        assert!(is_web_ui_path("/app/login"));
        assert!(!is_web_ui_path("/api/cfwdon/web/session"));
    }

    #[test]
    fn public_asset_paths_are_detected() {
        assert!(is_public_web_asset_path("/app/assets/index-abc.js"));
        assert!(!is_public_web_asset_path("/app/"));
    }

    #[test]
    fn accept_prefers_html_without_json() {
        assert!(accept_header_prefers_web_ui_html(
            "text/html,application/xhtml+xml"
        ));
        assert!(!accept_header_prefers_web_ui_html("application/json"));
    }
}
