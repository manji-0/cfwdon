use super::{Request, Response, Result, RouteContext, root_document};
use worker::ResponseBody;

const DEFAULT_WEB_UI_R2_PREFIX: &str = "phanpy";
const PHANPY_THEME_PATH: &str = "/cfwdon-phanpy-theme.css";
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

const PHANPY_THEME_CSS: &str = r#":root {
  --blue-color: #0f766e;
  --link-color: #0f766e;
  --link-bg-color: #0f766e18;
  --link-light-color: #14b8a699;
  --link-faded-color: #0f766e55;
  --button-bg-color: #0f766e;
  --button-bg-blur-color: #0f766eaa;
  --purple-color: #7c3aed;
  --violet-red-color: #db2777;
  --orange-color: #d97706;
  --red-color: #dc2626;
  --bg-color: #ffffff;
  --bg-faded-color: #f6f7f8;
  --bg-blur-color: #ffffffd9;
  --bg-faded-blur-color: #f6f7f8cc;
  --outline-color: rgba(15, 42, 48, 0.16);
  --outline-stronger-color: rgba(15, 42, 48, 0.32);
  --divider-color: rgba(15, 42, 48, 0.10);
  --drop-shadow-color: rgba(15, 42, 48, 0.16);
}

@media (prefers-color-scheme: dark) {
  :root {
    --blue-color: #2dd4bf;
    --link-color: #2dd4bf;
    --link-bg-color: #2dd4bf20;
    --link-light-color: #5eead499;
    --link-faded-color: #2dd4bf66;
    --button-bg-color: #14b8a6;
    --button-bg-blur-color: #14b8a6aa;
    --purple-color: #a78bfa;
    --violet-red-color: #f472b6;
    --orange-color: #fbbf24;
    --red-color: #f87171;
    --bg-color: #171a1c;
    --bg-faded-color: #101214;
    --bg-blur-color: #171a1cd9;
    --bg-faded-blur-color: #101214cc;
    --text-color: #f4f7f8;
    --text-insignificant-color: #f4f7f899;
    --outline-color: rgba(212, 232, 234, 0.16);
    --outline-stronger-color: rgba(212, 232, 234, 0.32);
    --divider-color: rgba(212, 232, 234, 0.10);
    --drop-shadow-color: rgba(0, 0, 0, 0.45);
  }
}

body {
  -webkit-font-smoothing: antialiased;
  text-rendering: optimizeLegibility;
}

.deck > header .header-grid {
  backdrop-filter: saturate(160%) blur(16px);
  background-color: var(--bg-blur-color);
}
"#;

pub(crate) async fn root_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if request_accepts_html(&req) {
        phanpy_html_response(ctx, "index.html").await
    } else {
        Response::from_json(&root_document())
    }
}

pub(crate) async fn fallback_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let path = url.path();

    if path == PHANPY_THEME_PATH {
        return phanpy_theme_response();
    }

    if is_reserved_backend_path(path) {
        return Response::error("not found", 404);
    }

    let object_key = phanpy_object_key_for_path(path);
    if let Some(object_key) = object_key.as_deref() {
        if let Some(response) = phanpy_static_response(&ctx, path, object_key).await? {
            return Ok(response);
        }
    }

    if request_accepts_html(&req) {
        return phanpy_html_response(ctx, "index.html").await;
    }

    Response::error("not found", 404)
}

async fn phanpy_html_response(ctx: RouteContext<()>, key: &str) -> Result<Response> {
    let Some((html, etag)) = load_phanpy_object_bytes(&ctx, key).await? else {
        return phanpy_missing_response();
    };
    let html = String::from_utf8(html)
        .map_err(|error| worker::Error::RustError(format!("invalid Phanpy HTML: {error}")))?;
    let html = configure_phanpy_html(&html);
    let mut response = Response::from_body(ResponseBody::Body(html.into_bytes()))?.with_status(200);
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-cache")?;
    response.headers_mut().set("ETag", &etag)?;
    Ok(response)
}

async fn phanpy_static_response(
    ctx: &RouteContext<()>,
    path: &str,
    key: &str,
) -> Result<Option<Response>> {
    let Some((body, etag)) = load_phanpy_object_bytes(ctx, key).await? else {
        return Ok(None);
    };

    if content_type_for_path(key) == "text/html; charset=utf-8" {
        let html = String::from_utf8(body)
            .map_err(|error| worker::Error::RustError(format!("invalid Phanpy HTML: {error}")))?;
        let html = configure_phanpy_html(&html);
        let mut response =
            Response::from_body(ResponseBody::Body(html.into_bytes()))?.with_status(200);
        response
            .headers_mut()
            .set("Content-Type", "text/html; charset=utf-8")?;
        response.headers_mut().set("Cache-Control", "no-cache")?;
        response.headers_mut().set("ETag", &etag)?;
        return Ok(Some(response));
    }

    let mut response = Response::from_body(ResponseBody::Body(body))?.with_status(200);
    response
        .headers_mut()
        .set("Content-Type", content_type_for_path(key))?;
    response
        .headers_mut()
        .set("Cache-Control", cache_control_for_path(path))?;
    response.headers_mut().set("ETag", &etag)?;

    Ok(Some(response))
}

async fn load_phanpy_object_bytes(
    ctx: &RouteContext<()>,
    key: &str,
) -> Result<Option<(Vec<u8>, String)>> {
    let config = super::load_config(ctx);
    let bucket = ctx.bucket(&config.media_binding)?;
    let object_key = format!("{}/{}", phanpy_r2_prefix(ctx), key.trim_start_matches('/'));
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

fn phanpy_r2_prefix(ctx: &RouteContext<()>) -> String {
    ctx.var("WEB_UI_R2_PREFIX")
        .ok()
        .map(|value| value.to_string())
        .map(|value| trim_object_key_segment(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_WEB_UI_R2_PREFIX.to_owned())
}

fn is_reserved_backend_path(path: &str) -> bool {
    RESERVED_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn phanpy_object_key_for_path(path: &str) -> Option<String> {
    let path = path.trim().trim_start_matches('/');
    if path.is_empty() {
        return Some("index.html".to_owned());
    }
    if path.ends_with('/') {
        return Some(format!("{path}index.html"));
    }
    if let Some(compose_asset) = path.strip_prefix("compose/") {
        if matches!(compose_asset, "manifest.webmanifest" | "sw.js") {
            return Some(compose_asset.to_owned());
        }
    }
    Some(path.to_owned())
}

fn trim_object_key_segment(value: &str) -> String {
    value.trim().trim_matches('/').to_owned()
}

fn inject_phanpy_theme(html: &str) -> String {
    if html.contains(PHANPY_THEME_PATH) {
        return html.to_owned();
    }

    let theme_link =
        r#"<link rel="stylesheet" href="/cfwdon-phanpy-theme.css" data-cfwdon-theme />"#;
    if let Some(head_end) = html.find("</head>") {
        return format!(
            "{}  {}\n{}",
            &html[..head_end],
            theme_link,
            &html[head_end..]
        );
    }

    format!("{theme_link}\n{html}")
}

fn configure_phanpy_html(html: &str) -> String {
    inject_phanpy_theme(html).replace(
        r#"<link rel="me" href="https://hachyderm.io/@phanpy" />"#,
        "",
    )
}

fn cache_control_for_path(path: &str) -> &'static str {
    if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn content_type_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "avif" => "image/avif",
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "html" => "text/html; charset=utf-8",
        "ico" => "image/x-icon",
        "jpg" | "jpeg" => "image/jpeg",
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
        _ => "application/octet-stream",
    }
}

fn phanpy_theme_response() -> Result<Response> {
    let mut response =
        Response::from_body(ResponseBody::Body(PHANPY_THEME_CSS.as_bytes().to_vec()))?
            .with_status(200);
    response
        .headers_mut()
        .set("Content-Type", "text/css; charset=utf-8")?;
    response
        .headers_mut()
        .set("Cache-Control", "public, max-age=3600")?;
    Ok(response)
}

fn phanpy_missing_response() -> Result<Response> {
    Response::error(
        "Phanpy bundle missing: upload Phanpy files to R2 under WEB_UI_R2_PREFIX",
        503,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_backend_paths_are_not_served_as_phanpy_fallback() {
        assert!(is_reserved_backend_path("/api/v1/unknown"));
        assert!(is_reserved_backend_path("/users/alice"));
        assert!(!is_reserved_backend_path("/settings"));
    }

    #[test]
    fn phanpy_object_key_maps_root_and_directory_indexes() {
        assert_eq!(
            phanpy_object_key_for_path("/"),
            Some("index.html".to_owned())
        );
        assert_eq!(
            phanpy_object_key_for_path("/compose/"),
            Some("compose/index.html".to_owned())
        );
        assert_eq!(
            phanpy_object_key_for_path("/assets/app.js"),
            Some("assets/app.js".to_owned())
        );
        assert_eq!(
            phanpy_object_key_for_path("/compose/manifest.webmanifest"),
            Some("manifest.webmanifest".to_owned())
        );
    }

    #[test]
    fn phanpy_html_injects_theme_stylesheet_once() {
        let html = "<html><head><title>Phanpy</title></head><body></body></html>";

        let configured = inject_phanpy_theme(html);
        let configured_again = inject_phanpy_theme(&configured);

        assert!(configured.contains(PHANPY_THEME_PATH));
        assert_eq!(
            configured.matches(PHANPY_THEME_PATH).count(),
            configured_again.matches(PHANPY_THEME_PATH).count()
        );
    }

    #[test]
    fn phanpy_html_removes_upstream_rel_me_link() {
        let html =
            r#"<html><head><link rel="me" href="https://hachyderm.io/@phanpy" /></head></html>"#;

        let configured = configure_phanpy_html(html);

        assert!(!configured.contains("hachyderm.io/@phanpy"));
        assert!(configured.contains(PHANPY_THEME_PATH));
    }

    #[test]
    fn phanpy_content_type_uses_common_static_types() {
        assert_eq!(
            content_type_for_path("/assets/application.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type_for_path("/assets/application.css"),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            content_type_for_path("/manifest.webmanifest"),
            "application/json; charset=utf-8"
        );
        assert_eq!(content_type_for_path("/og-image-2.jpg"), "image/jpeg");
    }

    #[test]
    fn phanpy_hashed_assets_are_cacheable() {
        assert_eq!(
            cache_control_for_path("/assets/application.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control_for_path("/sw.js"), "no-cache");
    }
}
