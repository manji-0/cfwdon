use crate::{
    add_log_message, log_json_event, observability_duration_ms, observability_started_at_ms,
    publish_d1_request_pressure, snapshot_d1_request_metrics,
};
use worker::{Env, Request, Response, Result};

pub(crate) struct HttpRequestContext {
    started_at_ms: f64,
    method: String,
    path: String,
    origin: Option<String>,
    upgrade: Option<String>,
    user_agent: String,
    log_api_requests: bool,
}

impl HttpRequestContext {
    pub(crate) fn from_request(req: &Request, env: &Env) -> Result<Self> {
        let request_url = req.url()?;

        Ok(Self {
            started_at_ms: observability_started_at_ms(),
            method: req.method().to_string(),
            path: request_url.path().to_owned(),
            origin: req.headers().get("Origin")?,
            upgrade: req.headers().get("Upgrade")?,
            user_agent: req.headers().get("User-Agent")?.unwrap_or_default(),
            log_api_requests: api_request_logging_enabled(env),
        })
    }

    pub(crate) fn method(&self) -> &str {
        &self.method
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn upgrade(&self) -> Option<&str> {
        self.upgrade.as_deref()
    }

    pub(crate) fn is_cors_preflight(&self) -> bool {
        self.method == "OPTIONS" && is_cors_enabled_path(&self.path)
    }

    pub(crate) fn cors_preflight_response(&self) -> Result<Response> {
        let mut response = Response::empty()?.with_status(204);
        apply_cors_headers(&mut response, self.origin.as_deref())?;
        response
            .headers_mut()
            .set("Access-Control-Max-Age", "86400")?;
        self.log_response(response)
    }

    pub(crate) fn finish_response(&self, mut response: Response) -> Result<Response> {
        if should_apply_cors_headers(
            &self.path,
            response.status_code(),
            response.headers().get("Upgrade")?.as_deref(),
            self.upgrade.as_deref(),
        ) {
            apply_cors_headers(&mut response, self.origin.as_deref())?;
        }
        self.log_response(response)
    }

    pub(crate) fn log_response(&self, response: Response) -> Result<Response> {
        log_api_request(
            self.log_api_requests,
            &self.method,
            &self.path,
            response.status_code(),
            &self.user_agent,
            observability_duration_ms(self.started_at_ms),
        );
        Ok(response)
    }
}

fn api_request_logging_enabled(env: &Env) -> bool {
    env.var("CFWDON_API_REQUEST_LOG")
        .ok()
        .map(|value| {
            let value = value.to_string();
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true)
}

fn log_api_request(
    enabled: bool,
    method: &str,
    path: &str,
    status: u16,
    user_agent: &str,
    duration_ms: u64,
) {
    if !enabled || !is_logged_api_path(path) {
        return;
    }
    let d1_metrics = snapshot_d1_request_metrics();
    publish_d1_request_pressure();
    let payload = add_log_message(
        serde_json::json!({
            "event": "api_request",
            "method": method,
            "path": path,
            "status": status,
            "duration_ms": duration_ms,
            "d1_query_count": d1_metrics.query_count,
            "d1_sql_ms_sum": d1_metrics.sql_ms_sum,
            "user_agent": sanitize_log_value(user_agent),
        }),
        format!(
            "API request {method} {path} completed with HTTP {status} in {duration_ms}ms (d1_queries={}, d1_sql_ms={})",
            d1_metrics.query_count, d1_metrics.sql_ms_sum
        ),
    );

    log_json_event(payload);
}

fn is_logged_api_path(path: &str) -> bool {
    path.starts_with("/api/") || path.starts_with("/oauth/") || path.starts_with("/.well-known/")
}

fn sanitize_log_value(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' | '\r' | '\t' => ' ',
            _ => character,
        })
        .take(200)
        .collect()
}

pub(crate) fn is_cors_enabled_path(path: &str) -> bool {
    path.starts_with("/api/")
        || path.starts_with("/oauth/")
        || path.starts_with("/media/")
        || path.starts_with("/profiles/")
        || path.starts_with("/users/")
        || path == "/.well-known/oauth-authorization-server"
        || path == "/.well-known/webfinger"
        || path == "/.well-known/host-meta"
        || path == "/.well-known/host-meta.json"
        || path == "/.well-known/nodeinfo"
        || path == "/nodeinfo/2.0"
        || path == "/nodeinfo/2.1"
}

fn should_apply_cors_headers(
    path: &str,
    status: u16,
    response_upgrade: Option<&str>,
    request_upgrade: Option<&str>,
) -> bool {
    is_cors_enabled_path(path)
        && !is_websocket_upgrade(status, response_upgrade)
        && !is_websocket_upgrade_header(request_upgrade)
}

/// Session cookies belong on HTML/API responses, not WebSocket upgrades.
/// Stream Hub 101 responses should not get `Set-Cookie` / `Cache-Control: no-store`.
/// Request `Upgrade` is enough: the DO proxy response can omit that header.
/// The next non-upgrade response still refreshes the Auth0 cookie session.
pub(crate) fn should_apply_auth0_web_session_cookies(
    status: u16,
    response_upgrade: Option<&str>,
    request_upgrade: Option<&str>,
) -> bool {
    !is_websocket_upgrade(status, response_upgrade) && !is_websocket_upgrade_header(request_upgrade)
}

fn is_websocket_upgrade(status: u16, upgrade_header: Option<&str>) -> bool {
    status == 101 || is_websocket_upgrade_header(upgrade_header)
}

fn is_websocket_upgrade_header(upgrade_header: Option<&str>) -> bool {
    upgrade_header
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
}

fn apply_cors_headers(response: &mut Response, origin: Option<&str>) -> Result<()> {
    response
        .headers_mut()
        .set("Access-Control-Allow-Origin", origin.unwrap_or("*"))?;
    response.headers_mut().set(
        "Access-Control-Allow-Methods",
        "GET,HEAD,POST,PUT,PATCH,DELETE,OPTIONS",
    )?;
    response.headers_mut().set(
        "Access-Control-Allow-Headers",
        "Authorization,Content-Type,Accept,Idempotency-Key",
    )?;
    response
        .headers_mut()
        .set("Access-Control-Expose-Headers", "Link,Authorization")?;
    response.headers_mut().set("Vary", "Origin")?;
    Ok(())
}

/// Plain-text MIME for error bodies that workers-rs leaves without Content-Type.
/// Missing Content-Type + nosniff makes Safari download the URL path segment
/// (e.g. `/oauth/auth0/callback` → file named `callback`).
pub(crate) const PLAIN_TEXT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

pub(crate) fn missing_content_type_fallback(
    status: u16,
    has_content_type: bool,
) -> Option<&'static str> {
    if has_content_type {
        return None;
    }
    (status == 404).then_some(PLAIN_TEXT_CONTENT_TYPE)
}

pub(crate) fn ensure_missing_content_type(mut response: Response) -> Result<Response> {
    let status = response.status_code();
    let has_content_type = response
        .headers()
        .get("Content-Type")?
        .is_some_and(|value| !value.trim().is_empty());
    if let Some(content_type) = missing_content_type_fallback(status, has_content_type) {
        response.headers_mut().set("Content-Type", content_type)?;
    }
    Ok(response)
}

pub(crate) fn error_response_with_plain_content_type(
    message: impl Into<String>,
    status: u16,
) -> Result<Response> {
    let mut response = Response::error(message, status)?;
    response
        .headers_mut()
        .set("Content-Type", PLAIN_TEXT_CONTENT_TYPE)?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::{
        PLAIN_TEXT_CONTENT_TYPE, is_cors_enabled_path, is_logged_api_path, is_websocket_upgrade,
        is_websocket_upgrade_header, missing_content_type_fallback, sanitize_log_value,
        should_apply_auth0_web_session_cookies, should_apply_cors_headers,
    };

    #[test]
    fn logged_api_path_scope_matches_browser_and_discovery_surfaces() {
        assert!(is_logged_api_path("/api/v1/statuses"));
        assert!(is_logged_api_path("/oauth/token"));
        assert!(is_logged_api_path("/.well-known/webfinger"));
        assert!(!is_logged_api_path("/users/alice"));
        assert!(!is_logged_api_path("/healthz"));
    }

    #[test]
    fn cors_enabled_paths_cover_discovery_surfaces() {
        assert!(is_cors_enabled_path("/.well-known/host-meta"));
        assert!(is_cors_enabled_path("/.well-known/host-meta.json"));
        assert!(is_cors_enabled_path("/.well-known/nodeinfo"));
        assert!(is_cors_enabled_path("/nodeinfo/2.0"));
        assert!(is_cors_enabled_path("/.well-known/webfinger"));
        assert!(!is_cors_enabled_path("/admin"));
    }

    #[test]
    fn log_value_sanitization_removes_line_breaks_and_caps_length() {
        let long_value = format!("agent\nwith\ttabs\r{}", "x".repeat(300));

        let sanitized = sanitize_log_value(&long_value);

        assert!(!sanitized.contains(['\n', '\r', '\t']));
        assert_eq!(sanitized.chars().count(), 200);
    }

    #[test]
    fn missing_content_type_fallback_only_for_bare_404() {
        assert_eq!(
            missing_content_type_fallback(404, false),
            Some(PLAIN_TEXT_CONTENT_TYPE)
        );
        assert_eq!(missing_content_type_fallback(404, true), None);
        assert_eq!(missing_content_type_fallback(500, false), None);
        assert_eq!(missing_content_type_fallback(200, false), None);
    }

    #[test]
    fn websocket_upgrade_detects_status_101() {
        assert!(is_websocket_upgrade(101, None));
    }

    #[test]
    fn websocket_upgrade_detects_upgrade_header() {
        assert!(is_websocket_upgrade(200, Some("websocket")));
        assert!(is_websocket_upgrade(200, Some("WebSocket")));
    }

    #[test]
    fn websocket_upgrade_rejects_normal_json_response() {
        assert!(!is_websocket_upgrade(200, None));
        assert!(!is_websocket_upgrade_header(None));
        assert!(!is_websocket_upgrade_header(Some("")));
        assert!(!is_websocket_upgrade_header(Some("h2c")));
    }

    #[test]
    fn cors_skipped_for_streaming_websocket_upgrade() {
        assert!(!should_apply_cors_headers(
            "/api/v1/streaming",
            101,
            Some("websocket"),
            None
        ));
    }

    #[test]
    fn cors_skipped_when_request_upgrades_to_websocket() {
        // Stream Hub DO proxy responses can be immutable and omit Upgrade.
        // Request Upgrade is enough to skip CORS mutation.
        assert!(!should_apply_cors_headers(
            "/api/v1/streaming",
            200,
            None,
            Some("websocket")
        ));
        assert!(!should_apply_cors_headers(
            "/api/v1/streaming",
            200,
            None,
            Some("WebSocket")
        ));
    }

    #[test]
    fn cors_applied_for_streaming_sse_without_websocket_upgrade() {
        assert!(should_apply_cors_headers(
            "/api/v1/streaming",
            200,
            None,
            None
        ));
    }

    #[test]
    fn cors_applied_for_normal_api_response() {
        assert!(should_apply_cors_headers(
            "/api/v1/timelines/public",
            200,
            None,
            None
        ));
    }

    #[test]
    fn auth0_session_cookies_skipped_for_websocket_upgrade() {
        assert!(!should_apply_auth0_web_session_cookies(
            101,
            Some("websocket"),
            None
        ));
        assert!(!should_apply_auth0_web_session_cookies(
            200,
            Some("WebSocket"),
            None
        ));
        assert!(!should_apply_auth0_web_session_cookies(
            200,
            None,
            Some("websocket")
        ));
    }

    #[test]
    fn auth0_session_cookies_applied_for_html_and_api_responses() {
        assert!(should_apply_auth0_web_session_cookies(200, None, None));
        assert!(should_apply_auth0_web_session_cookies(302, None, None));
    }
}
