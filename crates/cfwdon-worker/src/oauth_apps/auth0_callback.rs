use super::{
    Auth0TokenResponse, access_authenticated_without_account_response, access_token_cookie_max_age,
    auth0_authorize_state_cookie, auth0_domain_url, clear_auth0_authorize_state_cookie,
    constant_time_eq, oauth_authorize_error_response, redirect_response, set_auth0_session_cookies,
};
use crate::auth::find_account_by_email;
use crate::runtime_config::load_config;
use crate::verify_auth0_jwt;
use serde::Deserialize;
use worker::{Fetch, Headers, Method, Request, RequestInit, Response, Result, RouteContext};

#[derive(Debug, Deserialize)]
struct Auth0CallbackRequest {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub(crate) async fn auth0_callback_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match auth0_callback_response_inner(req, ctx).await {
        Ok(response) => Ok(response),
        Err(error) => {
            worker::console_error!("Auth0 callback failed: {error}");
            let (message, status) = auth0_callback_failure(&error);
            oauth_authorize_error_response(&message, status)
        }
    }
}

fn auth0_callback_failure(error: &worker::Error) -> (String, u16) {
    let detail = error.to_string();
    if detail.contains("Auth0 JWT email is not verified") {
        (
            "Please verify your email address in Auth0 before signing in".to_owned(),
            403,
        )
    } else {
        (auth0_callback_failure_message(error), 500)
    }
}

fn auth0_callback_failure_message(error: &worker::Error) -> String {
    format!("Auth0 login failed: {error}")
}

async fn auth0_callback_response_inner(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let callback = match req.query::<Auth0CallbackRequest>() {
        Ok(query) => query,
        Err(_) => return oauth_authorize_error_response("Invalid Auth0 callback request", 400),
    };
    if let Some(error) = callback.error.as_deref() {
        let description = callback
            .error_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(error);
        return oauth_authorize_error_response(description, 400);
    }
    let code = match callback
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(code) => code,
        None => {
            return oauth_authorize_error_response("Auth0 callback did not include a code", 400);
        }
    };
    let state = match callback
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(state) => state,
        None => return oauth_authorize_error_response("Auth0 callback did not include state", 400),
    };
    let Some(session) = auth0_authorize_state_cookie(&req)? else {
        return oauth_authorize_error_response("Missing Auth0 authorization state cookie", 400);
    };
    if !constant_time_eq(session.state.as_bytes(), state.as_bytes()) {
        return oauth_authorize_error_response("Auth0 authorization state mismatch", 400);
    }

    let mut callback_url = req.url()?;
    callback_url.set_path("/oauth/auth0/callback");
    callback_url.set_query(None);
    let token = exchange_auth0_authorization_code(
        &config,
        code,
        callback_url.as_str(),
        &session.code_verifier,
    )
    .await?;
    let claims = verify_auth0_jwt(&token.access_token, &config).await?;
    let email = claims
        .string_claim(&config.auth0_email_claim)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError(format!(
                "validated Auth0 JWT did not include a string {} claim",
                config.auth0_email_claim
            ))
        })?;
    if find_account_by_email(&db, &email).await?.is_none() {
        let mut response = access_authenticated_without_account_response(&config)?;
        clear_auth0_authorize_state_cookie(&mut response)?;
        return Ok(response);
    }

    let mut response = redirect_response(&session.return_url)?;
    set_auth0_session_cookies(
        &mut response,
        &token.access_token,
        token.refresh_token.as_deref(),
        access_token_cookie_max_age(token.expires_in),
    )?;
    clear_auth0_authorize_state_cookie(&mut response)?;
    Ok(response)
}

async fn exchange_auth0_authorization_code(
    config: &cfwdon_core::AppConfig,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<Auth0TokenResponse> {
    post_auth0_token_form(
        config,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", config.auth0_client_id.trim()),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ],
    )
    .await
}

pub(crate) async fn exchange_auth0_refresh_token(
    config: &cfwdon_core::AppConfig,
    refresh_token: &str,
) -> Result<Auth0TokenResponse> {
    post_auth0_token_form(
        config,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", config.auth0_client_id.trim()),
            ("refresh_token", refresh_token),
        ],
    )
    .await
}

async fn post_auth0_token_form(
    config: &cfwdon_core::AppConfig,
    pairs: &[(&str, &str)],
) -> Result<Auth0TokenResponse> {
    let mut token_url = auth0_domain_url(config).map_err(worker::Error::RustError)?;
    token_url.set_path("/oauth/token");
    token_url.set_query(None);
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        serializer.append_pair(name, value);
    }
    let body = serializer.finish();
    let headers = Headers::new();
    headers.set("Content-Type", "application/x-www-form-urlencoded")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body)));
    let request = Request::new_with_init(token_url.as_str(), &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(worker::Error::RustError(format!(
            "Auth0 token endpoint rejected request with HTTP {}",
            response.status_code()
        )));
    }
    response.json::<Auth0TokenResponse>().await
}

#[cfg(test)]
mod tests {
    use super::{auth0_callback_failure, auth0_callback_failure_message};

    #[test]
    fn auth0_callback_unverified_email_maps_to_http_403() {
        let (message, status) = auth0_callback_failure(&worker::Error::RustError(
            "Auth0 JWT email is not verified".to_owned(),
        ));
        assert_eq!(status, 403);
        assert!(message.contains("verify your email"));
        assert!(!message.contains("Auth0 JWT email is not verified"));
    }

    #[test]
    fn auth0_callback_other_errors_still_map_to_http_500() {
        let (message, status) = auth0_callback_failure(&worker::Error::RustError(
            "Auth0 JWT audience mismatch".to_owned(),
        ));
        assert_eq!(status, 500);
        assert!(message.starts_with("Auth0 login failed: "));
        assert!(message.contains("Auth0 JWT audience mismatch"));
    }

    #[test]
    fn auth0_callback_failure_message_is_browser_safe_html_source() {
        let message = auth0_callback_failure_message(&worker::Error::RustError(
            "Auth0 JWT audience mismatch".to_owned(),
        ));
        assert!(message.starts_with("Auth0 login failed: "));
        assert!(message.contains("Auth0 JWT audience mismatch"));
    }
}
