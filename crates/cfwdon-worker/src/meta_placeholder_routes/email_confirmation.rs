use super::account_registration::normalized_registration_email;
use super::{
    invalid_access_token_response, outside_authorized_scopes_response, validation_failed_response,
};
use crate::auth::{
    LocalApiAuthentication, authenticate_local_api_request, find_account_by_email,
    find_authenticated_local_account,
};
use crate::db_session::bind_request_d1;
use crate::id_utils::generate_entity_id;
use crate::identity::instance_base_url;
use crate::oauth_store::oauth_access_token_has_any_scope;
use crate::runtime_config::load_config;
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use std::collections::BTreeMap;
use worker::{
    Fetch, Headers, Method, Request, RequestInit, Response, ResponseBody, Result, RouteContext,
    d1::D1Type,
};

#[derive(Debug, Default, Deserialize)]
struct EmailConfirmationRequest {
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PendingEmailConfirmationRow {
    account_id: String,
    oauth_app_id: i64,
    pending_email: String,
}

#[derive(Debug, Default, Deserialize)]
struct EmailConfirmationQuery {
    confirmation_token: Option<String>,
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn email_confirmation_unavailable_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This method is only available while the e-mail is awaiting confirmation",
    }))?
    .with_status(403))
}

fn email_confirmation_application_mismatch_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This method is only available to the application the user originally signed-up with",
    }))?
    .with_status(403))
}

pub(in crate::meta_placeholder_routes) async fn upsert_pending_email_confirmation(
    db: &D1Database,
    account_id: &str,
    oauth_app_id: i64,
    pending_email: &str,
    confirmation_token: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Integer(i32::try_from(oauth_app_id).unwrap_or(i32::MAX)),
        D1Type::Text(pending_email),
        D1Type::Text(confirmation_token),
    ];
    db.prepare(
        "INSERT OR REPLACE INTO pending_email_confirmations (
            account_id,
            oauth_app_id,
            pending_email,
            confirmation_token,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(in crate::meta_placeholder_routes) async fn update_pending_email_confirmation_sent_at(
    db: &D1Database,
    account_id: &str,
    confirmation_token: &str,
) -> Result<()> {
    db.prepare(
        "UPDATE pending_email_confirmations
         SET confirmation_token = ?2,
             confirmation_sent_at = CURRENT_TIMESTAMP,
             updated_at = CURRENT_TIMESTAMP
         WHERE account_id = ?1",
    )
    .bind_refs(&[D1Type::Text(account_id), D1Type::Text(confirmation_token)])?
    .run()
    .await?;
    Ok(())
}

async fn find_pending_email_confirmation(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<PendingEmailConfirmationRow>> {
    let binding = D1Type::Text(account_id);
    db.prepare(
        "SELECT account_id,
                oauth_app_id,
                pending_email
         FROM pending_email_confirmations
         WHERE account_id = ?1
         LIMIT 1",
    )
    .bind_refs(&[binding])?
    .first::<PendingEmailConfirmationRow>(None)
    .await
}

async fn find_pending_email_confirmation_by_token(
    db: &D1Database,
    confirmation_token: &str,
) -> Result<Option<PendingEmailConfirmationRow>> {
    let binding = D1Type::Text(confirmation_token);
    db.prepare(
        "SELECT account_id,
                oauth_app_id,
                pending_email
         FROM pending_email_confirmations
         WHERE confirmation_token = ?1
         LIMIT 1",
    )
    .bind_refs(&[binding])?
    .first::<PendingEmailConfirmationRow>(None)
    .await
}

async fn confirm_pending_email_confirmation(
    db: &D1Database,
    pending: &PendingEmailConfirmationRow,
) -> Result<()> {
    db.prepare(
        "UPDATE accounts
         SET access_email = ?2
         WHERE id = ?1",
    )
    .bind_refs(&[
        D1Type::Text(pending.account_id.as_str()),
        D1Type::Text(pending.pending_email.as_str()),
    ])?
    .run()
    .await?;
    db.prepare("DELETE FROM pending_email_confirmations WHERE account_id = ?1")
        .bind_refs(&[D1Type::Text(pending.account_id.as_str())])?
        .run()
        .await?;
    Ok(())
}

async fn parse_email_confirmation_request(
    req: &mut Request,
) -> std::result::Result<EmailConfirmationRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<EmailConfirmationRequest>()
            .await
            .map_err(|error| format!("invalid JSON email confirmation payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form email confirmation payload: {error}"))?;
        EmailConfirmationRequest {
            email: form.get_field("email"),
        }
    };
    request.email = normalized_registration_email(request.email);
    if let Some(email) = request.email.as_deref()
        && !email.contains('@')
    {
        return Err("invalid email confirmation payload: email is invalid".to_owned());
    }
    Ok(request)
}

pub(crate) fn build_email_confirmation_url(
    config: &cfwdon_core::AppConfig,
    confirmation_token: &str,
) -> String {
    format!(
        "{}/auth/confirmation?confirmation_token={}",
        instance_base_url(config),
        urlencoding::encode(confirmation_token)
    )
}

pub(crate) fn build_email_confirmation_subject(config: &cfwdon_core::AppConfig) -> String {
    format!("Confirm your {} account", config.instance_name)
}

pub(crate) fn build_email_confirmation_text(
    config: &cfwdon_core::AppConfig,
    confirmation_url: &str,
) -> String {
    format!(
        "Welcome to {name}.\n\nConfirm your account by opening this link:\n{confirmation_url}\n\nIf you did not request this account, you can ignore this email.",
        name = config.instance_name,
    )
}

pub(crate) fn build_email_confirmation_html(
    config: &cfwdon_core::AppConfig,
    confirmation_url: &str,
) -> String {
    let name = html_escape(&config.instance_name);
    let confirmation_url = html_escape(confirmation_url);
    format!(
        "<p>Welcome to {name}.</p><p><a href=\"{confirmation_url}\">Confirm your account</a></p><p>If you did not request this account, you can ignore this email.</p>"
    )
}

pub(in crate::meta_placeholder_routes) async fn send_email_confirmation_message(
    ctx: &RouteContext<()>,
    config: &cfwdon_core::AppConfig,
    to_email: &str,
    confirmation_token: &str,
) -> Result<bool> {
    let Ok(api_key) = ctx.var("RESEND_API_KEY").map(|value| value.to_string()) else {
        return Ok(false);
    };
    let Ok(from_email) = ctx.var("EMAIL_FROM").map(|value| value.to_string()) else {
        return Ok(false);
    };
    let confirmation_url = build_email_confirmation_url(config, confirmation_token);
    let payload = serde_json::json!({
        "from": from_email,
        "to": [to_email],
        "subject": build_email_confirmation_subject(config),
        "text": build_email_confirmation_text(config, &confirmation_url),
        "html": build_email_confirmation_html(config, &confirmation_url),
    });
    let payload_json = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!("failed to encode email payload: {error}"))
    })?;

    let headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {api_key}"))?;
    headers.set("Content-Type", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload_json)));
    let request = Request::new_with_init("https://api.resend.com/emails", &init)?;
    let response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 == 2 {
        Ok(true)
    } else {
        Err(worker::Error::RustError(format!(
            "email provider rejected confirmation message with HTTP {}",
            response.status_code()
        )))
    }
}

fn email_confirmation_html_response(title: &str, message: &str, status: u16) -> Result<Response> {
    let title = html_escape(title);
    let message = html_escape(message);
    let mut response = Response::from_body(ResponseBody::Body(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"
    ).into_bytes()))?
    .with_status(status);
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response)
}

pub(crate) async fn create_email_confirmation_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let (account, request_oauth_app_id) =
        match authenticate_local_api_request(&req, &db, &config).await? {
            LocalApiAuthentication::OAuthToken(auth) => {
                if !oauth_access_token_has_any_scope(&auth.token, &["write:accounts", "write"]) {
                    return outside_authorized_scopes_response();
                }
                (auth.account, Some(auth.token.oauth_app_id))
            }
            LocalApiAuthentication::Auth0(account) => (account, None),
            LocalApiAuthentication::AppToken
            | LocalApiAuthentication::InvalidBearer
            | LocalApiAuthentication::None => {
                return invalid_access_token_response();
            }
        };
    let Some(pending) = find_pending_email_confirmation(&db, account.id()).await? else {
        return email_confirmation_unavailable_response();
    };
    if request_oauth_app_id.is_some_and(|oauth_app_id| oauth_app_id != pending.oauth_app_id) {
        return email_confirmation_application_mismatch_response();
    }
    let request = match parse_email_confirmation_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(&message, 422),
    };
    let confirmation_token = generate_entity_id(32)?;
    let pending_email = request.email.as_deref().unwrap_or(&pending.pending_email);
    if let Some(existing) = find_account_by_email(&db, pending_email).await?
        && existing.id() != account.id()
    {
        let mut details = BTreeMap::new();
        details.insert("email", vec!["has already been taken".to_owned()]);
        return validation_failed_response(details);
    }
    upsert_pending_email_confirmation(
        &db,
        account.id(),
        pending.oauth_app_id,
        pending_email,
        &confirmation_token,
    )
    .await?;
    if send_email_confirmation_message(&ctx, &config, pending_email, &confirmation_token).await? {
        update_pending_email_confirmation_sent_at(&db, account.id(), &confirmation_token).await?;
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn email_confirmation_page_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: EmailConfirmationQuery = req.query().unwrap_or_default();
    let Some(token) = query
        .confirmation_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return email_confirmation_html_response(
            "Confirmation token missing",
            "The confirmation link is missing a token.",
            422,
        );
    };
    let db = bind_request_d1(&ctx, &config)?;
    let Some(pending) = find_pending_email_confirmation_by_token(&db, token).await? else {
        return email_confirmation_html_response(
            "Confirmation token is invalid",
            "The confirmation link is invalid or has already been used.",
            404,
        );
    };
    if let Some(existing) = find_account_by_email(&db, &pending.pending_email).await?
        && existing.id() != pending.account_id
    {
        return email_confirmation_html_response(
            "Email is already taken",
            "The requested email address is already associated with another account.",
            409,
        );
    }
    confirm_pending_email_confirmation(&db, &pending).await?;
    email_confirmation_html_response(
        "Email confirmed",
        "Your email address has been confirmed. You can return to your app.",
        200,
    )
}

pub(crate) async fn check_email_confirmation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let Some(account) = find_authenticated_local_account(&req, &db, &config).await? else {
        return invalid_access_token_response();
    };
    let confirmed = find_pending_email_confirmation(&db, account.id())
        .await?
        .is_none()
        && !account.access_email().trim().is_empty();
    Response::from_json(&confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_core::AppConfig;

    #[test]
    fn email_confirmation_message_uses_configured_instance_and_token() {
        let config = AppConfig::new("social.example", "cfwdon", "test");
        let url = build_email_confirmation_url(&config, "tok en/1");

        assert_eq!(
            url,
            "https://social.example/auth/confirmation?confirmation_token=tok%20en%2F1"
        );
        assert_eq!(
            build_email_confirmation_subject(&config),
            "Confirm your cfwdon account"
        );
        assert!(build_email_confirmation_text(&config, &url).contains(&url));
        assert!(
            build_email_confirmation_html(&config, &url)
                .contains("https://social.example/auth/confirmation")
        );
    }
}
