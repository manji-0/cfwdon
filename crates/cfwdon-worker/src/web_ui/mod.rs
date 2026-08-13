mod assets {
    include!(concat!(env!("OUT_DIR"), "/web_ui_assets.rs"));
}

use crate::{
    auth0_login_redirect_response, auth0_logout_redirect_response, escape_html, instance_base_url,
    load_config,
};
use assets::lookup_web_embedded_asset;
use url::Url;
use worker::{Request, Response, ResponseBody, Result, RouteContext};

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
    let Some((bytes, content_type)) = lookup_web_embedded_asset(path) else {
        return Response::error("Not Found", 404);
    };
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
    let return_url = web_app_url(config, req)?;
    auth0_login_redirect_response(config, &return_url, &return_url)
}

pub(crate) fn web_app_url(config: &crate::AppConfig, req: &Request) -> Result<Url> {
    let request_url = req.url()?;
    web_app_url_from_request_url(&request_url, &instance_base_url(config))
}

fn web_app_url_from_request_url(request_url: &Url, instance_base_url: &str) -> Result<Url> {
    let mut redirect_url = request_url.clone();
    redirect_url.set_path("/app/");
    redirect_url.set_query(None);
    if !redirect_url
        .host_str()
        .is_some_and(|host| web_app_host_is_trusted(host, instance_base_url))
    {
        redirect_url = Url::parse(&format!("{instance_base_url}/app/"))
            .map_err(|error| worker::Error::RustError(format!("invalid web UI URL: {error}")))?;
    }
    Ok(redirect_url)
}

fn web_app_host_is_trusted(host: &str, instance_base_url: &str) -> bool {
    let instance_host = Url::parse(instance_base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
    if !instance_host.is_empty() && host.eq_ignore_ascii_case(&instance_host) {
        return true;
    }
    is_loopback_host(host)
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

pub(crate) fn web_ui_redirect_response(location: &str) -> Result<Response> {
    let escaped = escape_html(location);
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url={escaped}\"><title>Redirecting</title></head><body><main><p>Redirecting to <a href=\"{escaped}\">{escaped}</a>.</p></main></body></html>"
    );
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?.with_status(302);
    response.headers_mut().set("Location", location)?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::{
        accept_header_prefers_web_ui_html, is_public_web_asset_path, is_web_ui_path,
        web_app_url_from_request_url,
    };
    use url::Url;

    #[test]
    fn web_app_url_preserves_matching_instance_origin() {
        let request_url = Url::parse("https://social.example/timeline?ref=1").unwrap();
        let redirect_url =
            web_app_url_from_request_url(&request_url, "https://social.example").unwrap();
        assert_eq!(redirect_url.as_str(), "https://social.example/app/");
    }

    #[test]
    fn web_app_url_falls_back_to_configured_base_for_foreign_host() {
        let request_url = Url::parse("https://evil.example/timeline?ref=1").unwrap();
        let redirect_url =
            web_app_url_from_request_url(&request_url, "https://social.example").unwrap();
        assert_eq!(redirect_url.as_str(), "https://social.example/app/");
    }

    #[test]
    fn web_app_url_preserves_loopback_origin_for_local_dev() {
        let request_url = Url::parse("http://127.0.0.1:8787/timeline").unwrap();
        let redirect_url =
            web_app_url_from_request_url(&request_url, "https://social.example").unwrap();
        assert_eq!(redirect_url.as_str(), "http://127.0.0.1:8787/app/");
    }

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
