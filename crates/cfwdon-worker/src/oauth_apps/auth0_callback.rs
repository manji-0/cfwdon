use super::{
    access_authenticated_without_account_response, auth0_authorize_state_cookie,
    clear_auth0_authorize_state_cookie, constant_time_eq, oauth_authorize_error_response,
    redirect_response,
};
use crate::auth::{find_account_by_email, verify_auth0_jwt};
use crate::db_session::bind_request_d1;
use crate::oauth_store::{
    Auth0TokenResponse, access_token_cookie_max_age, post_auth0_token_form,
    set_auth0_session_cookies,
};
use crate::runtime_config::load_config;
use serde::Deserialize;
use worker::{Request, Response, Result, RouteContext};

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
    let db = bind_request_d1(&ctx, &config)?;
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
