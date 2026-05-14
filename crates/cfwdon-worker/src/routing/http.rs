use crate::{
    add_log_message, log_json_event, observability_duration_ms, observability_started_at_ms,
};
use worker::{Env, Request, Response, Result};

pub(crate) struct HttpRequestContext {
    started_at_ms: f64,
    method: String,
    path: String,
    origin: Option<String>,
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
        if is_cors_enabled_path(&self.path) {
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
    let payload = add_log_message(
        serde_json::json!({
            "event": "api_request",
            "method": method,
            "path": path,
            "status": status,
            "duration_ms": duration_ms,
            "user_agent": sanitize_log_value(user_agent),
        }),
        format!("API request {method} {path} completed with HTTP {status} in {duration_ms}ms"),
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
        || path == "/.well-known/oauth-authorization-server"
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

#[cfg(test)]
mod tests {
    use super::{is_logged_api_path, sanitize_log_value};

    #[test]
    fn logged_api_path_scope_matches_browser_and_discovery_surfaces() {
        assert!(is_logged_api_path("/api/v1/statuses"));
        assert!(is_logged_api_path("/oauth/token"));
        assert!(is_logged_api_path("/.well-known/webfinger"));
        assert!(!is_logged_api_path("/users/alice"));
        assert!(!is_logged_api_path("/healthz"));
    }

    #[test]
    fn log_value_sanitization_removes_line_breaks_and_caps_length() {
        let long_value = format!("agent\nwith\ttabs\r{}", "x".repeat(300));

        let sanitized = sanitize_log_value(&long_value);

        assert!(!sanitized.contains(['\n', '\r', '\t']));
        assert_eq!(sanitized.chars().count(), 200);
    }
}
