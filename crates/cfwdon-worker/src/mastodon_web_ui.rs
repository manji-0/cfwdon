use super::{AppConfig, Request, Response, Result, RouteContext, root_document};
use worker::ResponseBody;

const DEFAULT_WEB_UI_R2_PREFIX: &str = "webui";
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
        return web_ui_asset_response(ctx, path).await;
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
    let Some((html, etag)) = load_web_ui_object_bytes(&ctx, "index.html").await? else {
        return web_ui_missing_response();
    };
    let html = String::from_utf8(html)
        .map_err(|error| worker::Error::RustError(format!("invalid Web UI index HTML: {error}")))?;
    let html = configured_mastodon_web_ui_html(&html, &config);
    let mut response = Response::from_body(ResponseBody::Body(html.into_bytes()))?.with_status(200);
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-cache")?;
    response.headers_mut().set("ETag", &etag)?;
    Ok(response)
}

async fn web_ui_asset_response(ctx: RouteContext<()>, path: &str) -> Result<Response> {
    let config = super::load_config(&ctx);
    let object_key = web_ui_object_key_for_path(path);
    let Some((body, etag)) = load_web_ui_object_bytes(&ctx, &object_key).await? else {
        return Response::error("web ui asset not found", 404);
    };
    let body = configured_web_ui_asset_body(path, body, &config)?;

    let mut response = Response::from_body(ResponseBody::Body(body))?.with_status(200);
    response
        .headers_mut()
        .set("Content-Type", content_type_for_path(path))?;
    if is_cacheable_web_ui_asset(path) {
        response
            .headers_mut()
            .set("Cache-Control", "public, max-age=31536000, immutable")?;
    } else {
        response.headers_mut().set("Cache-Control", "no-cache")?;
    }
    response.headers_mut().set("ETag", &etag)?;

    Ok(response)
}

async fn load_web_ui_object_bytes(
    ctx: &RouteContext<()>,
    key: &str,
) -> Result<Option<(Vec<u8>, String)>> {
    let config = super::load_config(ctx);
    let bucket = ctx.bucket(&config.media_binding)?;
    let object_key = format!("{}/{}", web_ui_r2_prefix(ctx), key.trim_start_matches('/'));
    let Some(object) = bucket.get(&object_key).execute().await? else {
        return Ok(None);
    };
    let etag = object.http_etag();
    let Some(body) = object.body() else {
        return Ok(None);
    };
    Ok(Some((body.bytes().await?, etag)))
}

fn request_accepts_html(req: &Request) -> bool {
    req.headers()
        .get("Accept")
        .ok()
        .flatten()
        .map(|value| value.contains("text/html") || value.contains("application/xhtml+xml"))
        .unwrap_or(false)
}

fn web_ui_r2_prefix(ctx: &RouteContext<()>) -> String {
    ctx.var("WEB_UI_R2_PREFIX")
        .ok()
        .map(|value| value.to_string())
        .map(|value| trim_object_key_segment(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_WEB_UI_R2_PREFIX.to_owned())
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

fn web_ui_object_key_for_path(path: &str) -> String {
    trim_object_key_segment(path)
}

fn trim_object_key_segment(value: &str) -> String {
    value.trim().trim_matches('/').to_owned()
}

fn configured_mastodon_web_ui_html(html: &str, config: &AppConfig) -> String {
    let html = replace_initial_state(html, config);
    html.replace("https://mastodon.social/css/", "/css/")
        .replace("https://mastodon.social/packs/", "/packs/")
        .replace("https://mastodon.social", &instance_base_url(config))
}

fn configured_web_ui_asset_body(path: &str, body: Vec<u8>, config: &AppConfig) -> Result<Vec<u8>> {
    if path != "/manifest" {
        return Ok(body);
    }

    let text = String::from_utf8(body).map_err(|error| {
        worker::Error::RustError(format!("failed to decode Mastodon web manifest: {error}"))
    })?;
    Ok(text
        .replace("https://mastodon.social", &instance_base_url(config))
        .into_bytes())
}

fn replace_initial_state(html: &str, config: &AppConfig) -> String {
    let Some(script_start) = html.find("<script id=\"initial-state\"") else {
        return html.to_owned();
    };
    let Some(json_start_offset) = html[script_start..].find('>') else {
        return html.to_owned();
    };
    let json_start = script_start + json_start_offset + 1;
    let Some(json_end_offset) = html[json_start..].find("</script>") else {
        return html.to_owned();
    };
    let json_end = json_start + json_end_offset;

    let Ok(mut initial_state) =
        serde_json::from_str::<serde_json::Value>(&html[json_start..json_end])
    else {
        return html.to_owned();
    };
    apply_initial_state_config(&mut initial_state, config);
    let Ok(configured_json) = serde_json::to_string(&initial_state) else {
        return html.to_owned();
    };

    format!(
        "{}{}{}",
        &html[..json_start],
        configured_json,
        &html[json_end..]
    )
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

fn content_type_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "avif" => "image/avif",
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "html" => "text/html; charset=utf-8",
        "ico" => "image/x-icon",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "webmanifest" => "application/json; charset=utf-8",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "xml" => "application/xml; charset=utf-8",
        _ if path == "/manifest" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn web_ui_missing_response() -> Result<Response> {
    Response::error(
        "web ui bundle missing: upload Mastodon Web UI files to R2 under WEB_UI_R2_PREFIX",
        503,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_html_rewrites_initial_state_for_instance() {
        let mut config = AppConfig::new("fedi.manji.app", "cfwdon", "test instance");
        config.source_url = Some("https://github.com/manji-0/cfwdon".to_owned());
        let html = r#"<html><head><link rel="stylesheet" href="https://mastodon.social/css/custom.css"><meta content="https://mastodon.social/" property="og:url"><meta content="https://files.mastodon.social/site/logo.png" property="og:image"></head><body><script id="initial-state" type="application/json">{"meta":{"domain":"mastodon.social","title":"Mastodon","version":"4.6.0","source_url":"https://github.com/mastodon/mastodon","streaming_api_base_url":"wss://streaming.mastodon.social","registrations_open":true,"repository":"mastodon/mastodon"}}</script></body></html>"#;

        let configured = configured_mastodon_web_ui_html(html, &config);

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
    fn web_ui_object_key_trims_leading_slashes() {
        assert_eq!(web_ui_object_key_for_path("/packs/app.js"), "packs/app.js");
        assert_eq!(web_ui_object_key_for_path("packs/app.js"), "packs/app.js");
    }

    #[test]
    fn web_ui_content_type_uses_common_asset_types() {
        assert_eq!(
            content_type_for_path("/packs/application.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type_for_path("/packs/application.css"),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            content_type_for_path("/manifest"),
            "application/json; charset=utf-8"
        );
        assert_eq!(content_type_for_path("/packs/logo.svg"), "image/svg+xml");
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
