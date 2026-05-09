use super::{AppConfig, Request, Response, Result, RouteContext, root_document};
use worker::{Fetch, Headers, Method, RequestInit, ResponseBody};

const DEFAULT_MASTODON_WEB_UI_ORIGIN: &str = "https://mastodon.social";
const WEB_UI_ASSET_PREFIXES: &[&str] = &[
    "/assets/",
    "/css/",
    "/emoji/",
    "/headers/",
    "/avatars/",
    "/packs/",
    "/sounds/",
    "/system/",
];
const WEB_UI_ASSET_PATHS: &[&str] = &[
    "/apple-touch-icon.png",
    "/browserconfig.xml",
    "/favicon.ico",
    "/manifest",
    "/oops.png",
    "/robots.txt",
    "/sw.js",
];
const RESERVED_PREFIXES: &[&str] = &[
    "/.well-known/",
    "/api/",
    "/auth/",
    "/inbox",
    "/internal/",
    "/media/",
    "/nodeinfo/",
    "/oauth/",
    "/users/",
];

pub(crate) async fn root_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if request_accepts_html(&req) {
        web_ui_html_response(ctx).await
    } else {
        Response::from_json(&root_document())
    }
}

pub(crate) async fn fallback_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let path = url.path();

    if is_web_ui_asset_path(path) {
        return proxy_web_ui_path(ctx, &asset_request_target(&url)).await;
    }

    if is_reserved_backend_path(path) {
        return Response::error("not found", 404);
    }

    if request_accepts_html(&req) {
        return web_ui_html_response(ctx).await;
    }

    Response::error("not found", 404)
}

async fn web_ui_html_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = super::load_config(&ctx);
    let mut response = proxy_web_ui_path(ctx, "/").await?;
    let status = response.status_code();
    if status / 100 != 2 {
        return Ok(response);
    }

    let html = response.text().await?;
    let html = configured_mastodon_web_ui_html(&html, &config)?;
    let mut response =
        Response::from_body(ResponseBody::Body(html.into_bytes()))?.with_status(status);
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-cache")?;
    Ok(response)
}

async fn proxy_web_ui_path(ctx: RouteContext<()>, path_and_query: &str) -> Result<Response> {
    let config = super::load_config(&ctx);
    let origin = web_ui_origin(&ctx);
    let target = format!("{origin}{path_and_query}");
    let headers = Headers::new();
    headers.set("Accept", "*/*")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let request = worker::Request::new_with_init(&target, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    let content_type = response.headers().get("Content-Type")?;
    let cache_control = response.headers().get("Cache-Control")?;
    let etag = response.headers().get("ETag")?;
    let last_modified = response.headers().get("Last-Modified")?;
    let body = configured_web_ui_asset_body(path_and_query, response.bytes().await?, &config)?;

    let mut response = Response::from_body(ResponseBody::Body(body))?.with_status(status);
    if let Some(content_type) = content_type.as_deref() {
        response.headers_mut().set("Content-Type", content_type)?;
    }
    if is_cacheable_web_ui_asset(path_and_query) {
        response
            .headers_mut()
            .set("Cache-Control", "public, max-age=31536000, immutable")?;
    } else if let Some(cache_control) = cache_control.as_deref() {
        response.headers_mut().set("Cache-Control", cache_control)?;
    }
    if let Some(etag) = etag.as_deref() {
        response.headers_mut().set("ETag", etag)?;
    }
    if let Some(last_modified) = last_modified.as_deref() {
        response.headers_mut().set("Last-Modified", last_modified)?;
    }

    Ok(response)
}

fn request_accepts_html(req: &Request) -> bool {
    req.headers()
        .get("Accept")
        .ok()
        .flatten()
        .map(|value| value.contains("text/html") || value.contains("application/xhtml+xml"))
        .unwrap_or(false)
}

fn web_ui_origin(ctx: &RouteContext<()>) -> String {
    ctx.var("MASTODON_WEB_UI_ORIGIN")
        .ok()
        .map(|value| value.to_string())
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| value.starts_with("https://") && !value.is_empty())
        .unwrap_or_else(|| DEFAULT_MASTODON_WEB_UI_ORIGIN.to_owned())
}

fn is_web_ui_asset_path(path: &str) -> bool {
    WEB_UI_ASSET_PATHS.contains(&path)
        || WEB_UI_ASSET_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

fn is_cacheable_web_ui_asset(path: &str) -> bool {
    path.starts_with("/packs/")
        || path.starts_with("/assets/")
        || path.starts_with("/emoji/")
        || path.starts_with("/sounds/")
}

fn is_reserved_backend_path(path: &str) -> bool {
    RESERVED_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn asset_request_target(url: &url::Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    }
}

fn configured_mastodon_web_ui_html(html: &str, config: &AppConfig) -> Result<String> {
    let html = replace_initial_state(html, config)?;
    Ok(html
        .replace("https://mastodon.social/css/", "/css/")
        .replace("https://mastodon.social/packs/", "/packs/")
        .replace("https://mastodon.social", &instance_base_url(config)))
}

fn configured_web_ui_asset_body(
    path_and_query: &str,
    body: Vec<u8>,
    config: &AppConfig,
) -> Result<Vec<u8>> {
    if path_and_query != "/manifest" {
        return Ok(body);
    }

    let text = String::from_utf8(body).map_err(|error| {
        worker::Error::RustError(format!("failed to decode Mastodon web manifest: {error}"))
    })?;
    Ok(text
        .replace("https://mastodon.social", &instance_base_url(config))
        .into_bytes())
}

fn replace_initial_state(html: &str, config: &AppConfig) -> Result<String> {
    let Some(script_start) = html.find("<script id=\"initial-state\"") else {
        return Ok(html.to_owned());
    };
    let Some(json_start_offset) = html[script_start..].find('>') else {
        return Ok(html.to_owned());
    };
    let json_start = script_start + json_start_offset + 1;
    let Some(json_end_offset) = html[json_start..].find("</script>") else {
        return Ok(html.to_owned());
    };
    let json_end = json_start + json_end_offset;

    let mut initial_state: serde_json::Value = serde_json::from_str(&html[json_start..json_end])
        .map_err(|error| {
            worker::Error::RustError(format!("failed to parse Mastodon initial state: {error}"))
        })?;
    apply_initial_state_config(&mut initial_state, config);
    let configured_json = serde_json::to_string(&initial_state).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize Mastodon initial state: {error}"
        ))
    })?;

    Ok(format!(
        "{}{}{}",
        &html[..json_start],
        configured_json,
        &html[json_end..]
    ))
}

fn apply_initial_state_config(initial_state: &mut serde_json::Value, config: &AppConfig) {
    let meta = initial_state
        .as_object_mut()
        .and_then(|state| state.get_mut("meta"))
        .and_then(serde_json::Value::as_object_mut);
    let Some(meta) = meta else {
        return;
    };

    meta.insert(
        "domain".to_owned(),
        serde_json::Value::String(config.instance_domain.clone()),
    );
    meta.insert(
        "title".to_owned(),
        serde_json::Value::String(config.instance_name.clone()),
    );
    meta.insert(
        "version".to_owned(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").to_owned()),
    );
    meta.insert(
        "source_url".to_owned(),
        config
            .source_url
            .clone()
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
    );
    meta.insert(
        "streaming_api_base_url".to_owned(),
        serde_json::Value::String(format!("wss://{}/api/v1/streaming", config.instance_domain)),
    );
    meta.insert(
        "registrations_open".to_owned(),
        serde_json::Value::Bool(false),
    );
    meta.insert(
        "repository".to_owned(),
        serde_json::Value::String("manji-0/cfwdon".to_owned()),
    );
}

fn instance_base_url(config: &AppConfig) -> String {
    if config.instance_domain.starts_with("http://")
        || config.instance_domain.starts_with("https://")
    {
        config.instance_domain.trim_end_matches('/').to_owned()
    } else {
        format!("https://{}", config.instance_domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_html_rewrites_initial_state_for_instance() {
        let mut config = AppConfig::new("fedi.manji.app", "cfwdon", "test instance");
        config.source_url = Some("https://github.com/manji-0/cfwdon".to_owned());
        let html = r#"<html><head><link rel="stylesheet" href="https://mastodon.social/css/custom.css"><meta content="https://mastodon.social/" property="og:url"><meta content="https://files.mastodon.social/site/logo.png" property="og:image"></head><body><script id="initial-state" type="application/json">{"meta":{"domain":"mastodon.social","title":"Mastodon","version":"4.6.0","source_url":"https://github.com/mastodon/mastodon","streaming_api_base_url":"wss://streaming.mastodon.social","registrations_open":true,"repository":"mastodon/mastodon"}}</script></body></html>"#;

        let configured = configured_mastodon_web_ui_html(html, &config).unwrap();

        assert!(configured.contains("\"domain\":\"fedi.manji.app\""));
        assert!(configured.contains("\"title\":\"cfwdon\""));
        assert!(configured.contains("\"registrations_open\":false"));
        assert!(configured.contains("https://fedi.manji.app/"));
        assert!(configured.contains("href=\"/css/custom.css\""));
        assert!(configured.contains("https://files.mastodon.social/site/logo.png"));
        assert!(!configured.contains("https://mastodon.social"));
    }

    #[test]
    fn reserved_backend_paths_are_not_served_as_web_ui_fallback() {
        assert!(is_reserved_backend_path("/api/v1/unknown"));
        assert!(is_reserved_backend_path("/users/alice"));
        assert!(!is_reserved_backend_path("/deck/getting-started"));
    }

    #[test]
    fn asset_request_target_keeps_query_string() {
        let url = url::Url::parse("https://fedi.manji.app/packs/app.js?v=1").unwrap();
        assert_eq!(asset_request_target(&url), "/packs/app.js?v=1");
    }

    #[test]
    fn manifest_body_rewrites_instance_absolute_urls() {
        let config = AppConfig::new("fedi.manji.app", "cfwdon", "test instance");
        let body = br#"{"icons":[{"src":"https://mastodon.social/packs/icon.png"}]}"#.to_vec();

        let configured = configured_web_ui_asset_body("/manifest", body, &config).unwrap();
        let configured = String::from_utf8(configured).unwrap();

        assert!(configured.contains("https://fedi.manji.app/packs/icon.png"));
        assert!(!configured.contains("https://mastodon.social"));
    }
}
