use super::authorize_validation::{code_challenge_method_is_supported, validate_authorize_request};
use super::{
    OAUTH_AUTHORIZE_CSRF_COOKIE, OAuthAppRow, OAuthAuthorizeFailure, OAuthAuthorizeRequest,
    auth0_login_configured, auth0_login_redirect_response, auth0_logout_url,
    authorization_redirect_with_params, constant_time_eq, escape_html, html_response,
    issue_oauth_authorization_code, load_account_password_hash, oauth_authorize_url_from_form,
    redirect_response, request_cookie_value, verify_account_password_hash,
};
use crate::auth::{find_account_by_email, find_account_by_username};
use crate::id_utils::generate_entity_id;
use crate::runtime_config::load_config;
use url::Url;
use worker::{Request, Response, Result, RouteContext};

use crate::D1Database;

#[derive(Debug, Default)]
struct OAuthAuthorizeLoginRequest {
    username: Option<String>,
    password: Option<String>,
    approve: bool,
    csrf_token: Option<String>,
    authorize: OAuthAuthorizeRequest,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Auth0AuthorizeGetAction {
    RedirectToLogin,
    MissingLinkedAccount,
    ShowConsent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Auth0AuthorizePostAction {
    RedirectToLogin,
    ShowConsent,
    IssueCode,
}
const AUTHORIZE_PAGE_STYLE: &str = r#"
:root {
    color-scheme: light;
    --bg: #f6f8fb;
    --panel: #ffffff;
    --text: #172033;
    --muted: #5f6b7a;
    --border: #d9e0ea;
    --accent: #2f6fed;
    --accent-hover: #1f5fd4;
    --danger-bg: #fff1f2;
    --danger-text: #9f1239;
    --danger-border: #fecdd3;
}
* { box-sizing: border-box; }
body {
    margin: 0;
    min-height: 100vh;
    background: var(--bg);
    color: var(--text);
    font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
main {
    width: min(100%, 560px);
    margin: 0 auto;
    padding: 56px 20px;
}
.authorize-panel {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 18px 48px rgba(23, 32, 51, 0.10);
    overflow: hidden;
}
.authorize-header {
    padding: 32px 32px 24px;
    border-bottom: 1px solid var(--border);
}
.eyebrow {
    margin: 0 0 10px;
    color: var(--muted);
    font-size: 0.82rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
}
h1 {
    margin: 0;
    font-size: clamp(1.55rem, 4vw, 2.15rem);
    line-height: 1.12;
}
.summary {
    margin: 12px 0 0;
    color: var(--muted);
    line-height: 1.55;
}
form { padding: 28px 32px 32px; }
.error {
    margin: 0 0 18px;
    padding: 12px 14px;
    border: 1px solid var(--danger-border);
    border-radius: 8px;
    background: var(--danger-bg);
    color: var(--danger-text);
}
dl {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr);
    gap: 14px 18px;
    margin: 0 0 24px;
}
dt {
    color: var(--muted);
    font-size: 0.86rem;
    font-weight: 700;
}
dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
}
.scope-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
}
.scope-pill {
    display: inline-flex;
    align-items: center;
    min-height: 28px;
    padding: 3px 10px;
    border-radius: 999px;
    background: #eef4ff;
    color: #1f4e9d;
    font-size: 0.88rem;
    font-weight: 700;
}
.redirect-uri {
    display: block;
    padding: 9px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: #f8fafc;
    color: #25324a;
    font-family: ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace;
    font-size: 0.88rem;
}
.field-stack {
    display: grid;
    gap: 16px;
    margin-bottom: 22px;
}
label {
    display: grid;
    gap: 7px;
    color: var(--muted);
    font-size: 0.9rem;
    font-weight: 700;
}
input:not([type="hidden"]) {
    width: 100%;
    min-height: 42px;
    padding: 9px 11px;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text);
    font: inherit;
}
input:not([type="hidden"]):focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgba(47, 111, 237, 0.16);
    outline: none;
}
.actions {
    display: flex;
    justify-content: flex-end;
}
button {
    min-height: 44px;
    padding: 0 18px;
    border: 0;
    border-radius: 8px;
    background: var(--accent);
    color: white;
    cursor: pointer;
    font: inherit;
    font-weight: 800;
}
button:hover { background: var(--accent-hover); }
@media (max-width: 560px) {
    main { padding: 20px 12px; }
    .authorize-header,
    form {
        padding-left: 20px;
        padding-right: 20px;
    }
    dl {
        grid-template-columns: 1fr;
        gap: 6px;
    }
    dt { margin-top: 8px; }
    .actions,
    button { width: 100%; }
}
"#;
fn access_login_redirect_from_authorize_request(
    config: &cfwdon_core::AppConfig,
    base_url: &Url,
    authorize: &OAuthAuthorizeRequest,
) -> Result<Response> {
    let return_url =
        oauth_authorize_url_from_form(base_url, authorize).map_err(worker::Error::RustError)?;
    auth0_login_redirect_response(config, base_url, &return_url)
}
pub(in crate::oauth_apps) fn access_authenticated_without_account_response(
    config: &cfwdon_core::AppConfig,
) -> Result<Response> {
    let logout_url = auth0_logout_url(config)
        .map(|url| url.to_string())
        .unwrap_or_default();
    Ok(Response::from_json(&serde_json::json!({
        "error": "Auth0 authentication succeeded, but no local account is registered for this email.",
        "registration_url": format!("{}/auth/sign_up", crate::instance_base_url(config)),
        "logout_url": logout_url,
    }))?
    .with_status(403))
}
fn oauth_authorize_csrf_cookie(req: &Request) -> Result<Option<String>> {
    request_cookie_value(req, OAUTH_AUTHORIZE_CSRF_COOKIE)
}
fn oauth_authorize_csrf_matches(req: &Request, submitted_token: Option<&str>) -> Result<bool> {
    let Some(submitted_token) = submitted_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    Ok(oauth_authorize_csrf_cookie(req)?
        .as_deref()
        .is_some_and(|cookie_token| {
            constant_time_eq(cookie_token.as_bytes(), submitted_token.as_bytes())
        }))
}

fn set_oauth_authorize_csrf_cookie(response: &mut Response, token: &str) -> Result<()> {
    response.headers_mut().append(
        "Set-Cookie",
        &format!(
            "{OAUTH_AUTHORIZE_CSRF_COOKIE}={token}; Path=/oauth/authorize; HttpOnly; SameSite=Lax; Secure"
        ),
    )?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(())
}
fn oauth_authorize_consent_response(
    request: &OAuthAuthorizeRequest,
    app: &OAuthAppRow,
    error: Option<&str>,
    require_login_credentials: bool,
    status: u16,
) -> Result<Response> {
    let csrf_token = generate_entity_id(32)?;
    let body =
        oauth_authorize_page_body(request, app, error, require_login_credentials, &csrf_token);
    let mut response = html_response(&body, status)?;
    set_oauth_authorize_csrf_cookie(&mut response, &csrf_token)?;
    Ok(response)
}

pub(in crate::oauth_apps) fn oauth_authorize_error_response(
    message: &str,
    status: u16,
) -> Result<Response> {
    html_response(
        &format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Authorization error</title></head><body><main><h1>Authorization error</h1><p>{}</p></main></body></html>",
            escape_html(message)
        ),
        status,
    )
}

fn oauth_authorize_failure_response(failure: OAuthAuthorizeFailure) -> Result<Response> {
    match failure {
        OAuthAuthorizeFailure::Html { message } => oauth_authorize_error_response(&message, 400),
        OAuthAuthorizeFailure::Redirect {
            redirect_uri,
            state,
            error,
            description,
        } => oauth_authorize_error_redirect(&redirect_uri, state.as_deref(), error, &description),
    }
}

fn oauth_authorize_error_redirect(
    redirect_uri: &str,
    state: Option<&str>,
    error: &str,
    description: &str,
) -> Result<Response> {
    if redirect_uri == "urn:ietf:wg:oauth:2.0:oob" {
        return oauth_authorize_error_response(description, 400);
    }
    let location =
        build_oauth_authorize_error_redirect_url(redirect_uri, state, error, description)?;
    redirect_response(&location)
}

fn build_oauth_authorize_error_redirect_url(
    redirect_uri: &str,
    state: Option<&str>,
    error: &str,
    description: &str,
) -> Result<String> {
    let mut url = Url::parse(redirect_uri)
        .map_err(|err| worker::Error::RustError(format!("invalid redirect URI: {err}")))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("error", error);
        if !description.is_empty() {
            query.append_pair("error_description", description);
        }
        if let Some(state) = state.filter(|value| !value.is_empty()) {
            query.append_pair("state", state);
        }
    }
    Ok(url.to_string())
}

#[cfg_attr(not(test), allow(dead_code))]
fn authorize_failure_should_use_html_page(failure: &OAuthAuthorizeFailure) -> bool {
    matches!(failure, OAuthAuthorizeFailure::Html { .. })
}

fn access_authorize_get_action(
    has_authenticated_local_account: bool,
    has_authenticated_access_user_without_account: bool,
) -> Auth0AuthorizeGetAction {
    if has_authenticated_local_account {
        Auth0AuthorizeGetAction::ShowConsent
    } else if has_authenticated_access_user_without_account {
        Auth0AuthorizeGetAction::MissingLinkedAccount
    } else {
        Auth0AuthorizeGetAction::RedirectToLogin
    }
}

fn access_authorize_post_action(
    has_authenticated_local_account: bool,
    approved: bool,
    has_valid_credentials: bool,
) -> Auth0AuthorizePostAction {
    if !has_authenticated_local_account {
        Auth0AuthorizePostAction::RedirectToLogin
    } else if approved || has_valid_credentials {
        Auth0AuthorizePostAction::IssueCode
    } else {
        Auth0AuthorizePostAction::ShowConsent
    }
}

fn oauth_authorize_page_body(
    request: &OAuthAuthorizeRequest,
    app: &OAuthAppRow,
    error: Option<&str>,
    require_login_credentials: bool,
    csrf_token: &str,
) -> String {
    let hidden = [
        ("response_type", request.response_type.as_deref()),
        ("client_id", request.client_id.as_deref()),
        ("redirect_uri", request.redirect_uri.as_deref()),
        ("scope", request.scope.as_deref()),
        ("state", request.state.as_deref()),
        ("code_challenge", request.code_challenge.as_deref()),
        (
            "code_challenge_method",
            request.code_challenge_method.as_deref(),
        ),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
        value.map(|value| {
            format!(
                "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
                escape_html(name),
                escape_html(value)
            )
        })
    })
    .collect::<Vec<_>>()
    .join("\n");
    let csrf_input = format!(
        "<input type=\"hidden\" name=\"csrf_token\" value=\"{}\">",
        escape_html(csrf_token)
    );
    let error_html = error
        .map(|message| {
            format!(
                "<p class=\"error\" role=\"alert\">{}</p>",
                escape_html(message)
            )
        })
        .unwrap_or_default();
    let scope_text = request
        .scope
        .as_deref()
        .map(escape_html)
        .unwrap_or_else(|| "read".to_owned());
    let scope_pills = scope_text
        .split_whitespace()
        .map(|scope| format!("<span class=\"scope-pill\">{scope}</span>"))
        .collect::<Vec<_>>()
        .join("");
    let scope_html = if scope_pills.is_empty() {
        scope_text
    } else {
        format!("<span class=\"scope-list\">{scope_pills}</span>")
    };
    let redirect_uri_html = request
        .redirect_uri
        .as_deref()
        .map(escape_html)
        .unwrap_or_default();
    let app_details = format!(
        "<dl><dt>Application</dt><dd>{}</dd><dt>Scopes</dt><dd>{scope_html}</dd><dt>Redirect URI</dt><dd><span class=\"redirect-uri\">{redirect_uri_html}</span></dd></dl>",
        escape_html(&app.name)
    );
    let form_body = if require_login_credentials {
        format!(
            "{app_details}{hidden}{csrf_input}<div class=\"field-stack\"><label>Username or email <input name=\"username\" autocomplete=\"username\" required></label><label>Password <input name=\"password\" type=\"password\" autocomplete=\"current-password\" required></label></div><div class=\"actions\"><button type=\"submit\">Authorize</button></div>"
        )
    } else {
        format!(
            "{app_details}<p class=\"summary\">Approve access for this application.</p>{hidden}{csrf_input}<input type=\"hidden\" name=\"approve\" value=\"true\"><div class=\"actions\"><button type=\"submit\">Authorize</button></div>"
        )
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Authorize {app}</title><style>{style}</style></head><body><main><section class=\"authorize-panel\"><header class=\"authorize-header\"><p class=\"eyebrow\">cfwdon authorization</p><h1>Authorize {app}</h1><p class=\"summary\">Review the permissions requested by this application before continuing.</p></header><form method=\"post\" action=\"/oauth/authorize\">{error}{form_body}</form></section></main></body></html>",
        app = escape_html(&app.name),
        style = AUTHORIZE_PAGE_STYLE,
        error = error_html,
        form_body = form_body,
    )
}

fn oauth_login_page(
    request: &OAuthAuthorizeRequest,
    app: &OAuthAppRow,
    error: Option<&str>,
) -> Result<Response> {
    oauth_authorize_consent_response(request, app, error, true, error.map(|_| 401).unwrap_or(200))
}
#[cfg_attr(not(test), allow(dead_code))]
fn normalize_authorize_request(
    request: OAuthAuthorizeRequest,
) -> std::result::Result<OAuthAuthorizeRequest, String> {
    let response_type = request
        .response_type
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "code".to_owned());
    if response_type != "code" {
        return Err("Only response_type=code is supported".to_owned());
    }
    let client_id = request
        .client_id
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "client_id is required".to_owned())?;
    let redirect_uri = request
        .redirect_uri
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "redirect_uri is required".to_owned())?;
    let code_challenge_method = request
        .code_challenge_method
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let code_challenge = request
        .code_challenge
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if code_challenge.is_some()
        && !code_challenge_method_is_supported(code_challenge_method.as_deref())
    {
        return Err("unsupported code_challenge_method".to_owned());
    }
    Ok(OAuthAuthorizeRequest {
        response_type: Some(response_type),
        client_id: Some(client_id),
        redirect_uri: Some(redirect_uri),
        scope: request.scope.map(|value| value.trim().to_owned()),
        state: request.state.map(|value| value.trim().to_owned()),
        code_challenge,
        code_challenge_method,
    })
}

async fn parse_oauth_authorize_login_request(
    req: &mut Request,
) -> std::result::Result<OAuthAuthorizeLoginRequest, String> {
    let form = req
        .form_data()
        .await
        .map_err(|error| format!("invalid authorization form payload: {error}"))?;
    Ok(OAuthAuthorizeLoginRequest {
        username: form.get_field("username"),
        password: form.get_field("password"),
        approve: form
            .get_field("approve")
            .map(|value| value.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false),
        csrf_token: form.get_field("csrf_token"),
        authorize: OAuthAuthorizeRequest {
            response_type: form.get_field("response_type"),
            client_id: form.get_field("client_id"),
            redirect_uri: form.get_field("redirect_uri"),
            scope: form.get_field("scope"),
            state: form.get_field("state"),
            code_challenge: form.get_field("code_challenge"),
            code_challenge_method: form.get_field("code_challenge_method"),
        },
    })
}
async fn authorize_account_by_password(
    db: &D1Database,
    username_or_email: Option<String>,
    password: Option<String>,
) -> Result<Option<cfwdon_domain::LocalAccount>> {
    let Some(username_or_email) = username_or_email
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some(password) = password.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let account = if username_or_email.contains('@') {
        find_account_by_email(db, &username_or_email).await?
    } else {
        find_account_by_username(db, &username_or_email.to_ascii_lowercase()).await?
    };
    let Some(account) = account else {
        return Ok(None);
    };
    let Some(password_hash) = load_account_password_hash(db, account.id()).await? else {
        return Ok(None);
    };
    if verify_account_password_hash(&password, &password_hash) {
        Ok(Some(account))
    } else {
        Ok(None)
    }
}

async fn redirect_with_authorization_code(
    db: &D1Database,
    request: &OAuthAuthorizeRequest,
    app: &OAuthAppRow,
    account_id: &str,
    scopes: &[String],
) -> Result<Response> {
    let redirect_uri = request.redirect_uri.as_deref().ok_or_else(|| {
        worker::Error::RustError("validated authorization request missing redirect_uri".to_owned())
    })?;
    let code = issue_oauth_authorization_code(
        db,
        app.id,
        account_id,
        redirect_uri,
        scopes,
        request.code_challenge.as_deref(),
        request.code_challenge_method.as_deref(),
    )
    .await?;
    let mut params = vec![("code", code)];
    if let Some(state) = request.state.as_ref() {
        params.push(("state", state.clone()));
    }
    authorization_redirect_with_params(redirect_uri, &params)
}

pub(crate) async fn oauth_authorize_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    if req.method().as_ref() == "POST" {
        let login = match parse_oauth_authorize_login_request(&mut req).await {
            Ok(login) => login,
            Err(message) => return Response::error(&message, 422),
        };
        let (authorize, app, scopes) = match validate_authorize_request(&db, login.authorize).await
        {
            Ok(value) => value,
            Err(failure) => return oauth_authorize_failure_response(failure),
        };
        if auth0_login_configured(&config) {
            let base_url = req.url()?;
            let authenticated_account =
                crate::find_authenticated_local_account(&req, &db, &config).await?;
            let password_account =
                authorize_account_by_password(&db, login.username, login.password).await?;
            let csrf_valid = oauth_authorize_csrf_matches(&req, login.csrf_token.as_deref())?;
            return match access_authorize_post_action(
                authenticated_account.is_some(),
                login.approve,
                password_account.is_some(),
            ) {
                Auth0AuthorizePostAction::RedirectToLogin => {
                    access_login_redirect_from_authorize_request(&config, &base_url, &authorize)
                }
                Auth0AuthorizePostAction::ShowConsent => {
                    oauth_authorize_consent_response(&authorize, &app, None, false, 200)
                }
                Auth0AuthorizePostAction::IssueCode => {
                    if !csrf_valid {
                        return Response::error("Invalid OAuth authorization CSRF token.", 403);
                    }
                    let Some(account) = password_account.or(authenticated_account) else {
                        return Response::error("Authenticated account is required.", 401);
                    };
                    redirect_with_authorization_code(&db, &authorize, &app, account.id(), &scopes)
                        .await
                }
            };
        }
        if !oauth_authorize_csrf_matches(&req, login.csrf_token.as_deref())? {
            return Response::error("Invalid OAuth authorization CSRF token.", 403);
        }
        let authenticated_account =
            crate::find_authenticated_local_account(&req, &db, &config).await?;
        let account = if login.approve {
            authenticated_account
        } else {
            None
        }
        .or(authorize_account_by_password(&db, login.username, login.password).await?);
        let Some(account) = account else {
            return oauth_login_page(&authorize, &app, Some("Invalid username or password."));
        };
        return redirect_with_authorization_code(&db, &authorize, &app, account.id(), &scopes)
            .await;
    }

    let authorize = match req.query::<OAuthAuthorizeRequest>() {
        Ok(query) => query,
        Err(_) => return oauth_authorize_error_response("Invalid authorization request", 400),
    };
    let (authorize, app, _scopes) = match validate_authorize_request(&db, authorize).await {
        Ok(value) => value,
        Err(failure) => return oauth_authorize_failure_response(failure),
    };
    let authenticated_account = crate::find_authenticated_local_account(&req, &db, &config).await?;
    if auth0_login_configured(&config) {
        let action = access_authorize_get_action(
            authenticated_account.is_some(),
            crate::extract_authenticated_user(&req, &config)
                .await?
                .is_some(),
        );
        return match action {
            Auth0AuthorizeGetAction::RedirectToLogin => {
                access_login_redirect_from_authorize_request(&config, &req.url()?, &authorize)
            }
            Auth0AuthorizeGetAction::MissingLinkedAccount => {
                access_authenticated_without_account_response(&config)
            }
            Auth0AuthorizeGetAction::ShowConsent => {
                oauth_authorize_consent_response(&authorize, &app, None, false, 200)
            }
        };
    }
    if let Some(account) = authenticated_account {
        let _ = account;
        return oauth_authorize_consent_response(&authorize, &app, None, false, 200);
    }
    oauth_login_page(&authorize, &app, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth_app_fixture() -> OAuthAppRow {
        OAuthAppRow {
            id: 7,
            name: "Client".to_owned(),
            website: None,
            scopes_json: "[\"read\"]".to_owned(),
            redirect_uri_legacy: "https://client.example/callback".to_owned(),
            redirect_uris_json: "[\"https://client.example/callback\"]".to_owned(),
            client_id: "client".to_owned(),
            client_secret: "secret".to_owned(),
            client_secret_expires_at: 0,
        }
    }

    #[test]
    fn access_authorize_actions_match_expected_flow() {
        assert_eq!(
            access_authorize_get_action(false, false),
            Auth0AuthorizeGetAction::RedirectToLogin
        );
        assert_eq!(
            access_authorize_get_action(false, true),
            Auth0AuthorizeGetAction::MissingLinkedAccount
        );
        assert_eq!(
            access_authorize_get_action(true, false),
            Auth0AuthorizeGetAction::ShowConsent
        );

        assert_eq!(
            access_authorize_post_action(false, false, false),
            Auth0AuthorizePostAction::RedirectToLogin
        );
        assert_eq!(
            access_authorize_post_action(true, false, false),
            Auth0AuthorizePostAction::ShowConsent
        );
        assert_eq!(
            access_authorize_post_action(true, true, false),
            Auth0AuthorizePostAction::IssueCode
        );
        assert_eq!(
            access_authorize_post_action(true, false, true),
            Auth0AuthorizePostAction::IssueCode
        );
    }

    #[test]
    fn oauth_authorize_page_body_requires_credentials_for_login_flow() {
        let body = oauth_authorize_page_body(
            &OAuthAuthorizeRequest {
                response_type: Some("code".to_owned()),
                client_id: Some("client".to_owned()),
                redirect_uri: Some("https://client.example/callback".to_owned()),
                scope: Some("read".to_owned()),
                state: Some("state-1".to_owned()),
                code_challenge: None,
                code_challenge_method: None,
            },
            &oauth_app_fixture(),
            None,
            true,
            "csrf-1",
        );

        assert!(body.contains("name=\"username\""));
        assert!(body.contains("type=\"password\""));
        assert!(body.contains("name=\"csrf_token\" value=\"csrf-1\""));
        assert!(body.contains("<dt>Redirect URI</dt>"));
        assert!(
            body.contains("<span class=\"redirect-uri\">https://client.example/callback</span>")
        );
        assert!(body.contains("<button type=\"submit\">Authorize</button>"));
    }

    #[test]
    fn oauth_authorize_page_body_shows_authenticated_consent_without_credentials() {
        let body = oauth_authorize_page_body(
            &OAuthAuthorizeRequest {
                response_type: Some("code".to_owned()),
                client_id: Some("client".to_owned()),
                redirect_uri: Some("https://client.example/callback".to_owned()),
                scope: Some("read write".to_owned()),
                state: Some("state-1".to_owned()),
                code_challenge: Some("challenge-1".to_owned()),
                code_challenge_method: Some("S256".to_owned()),
            },
            &oauth_app_fixture(),
            None,
            false,
            "csrf-1",
        );

        assert!(!body.contains("name=\"username\""));
        assert!(!body.contains("type=\"password\""));
        assert!(body.contains("Authorize Client"));
        assert!(body.contains("name=\"client_id\" value=\"client\""));
        assert!(body.contains("name=\"scope\" value=\"read write\""));
        assert!(body.contains("name=\"code_challenge_method\" value=\"S256\""));
        assert!(body.contains("name=\"approve\" value=\"true\""));
        assert!(body.contains("name=\"csrf_token\" value=\"csrf-1\""));
        assert!(body.contains("<dt>Scopes</dt>"));
        assert!(body.contains("<span class=\"scope-pill\">read</span>"));
        assert!(body.contains("<span class=\"scope-pill\">write</span>"));
        assert!(body.contains("Approve access for this application"));
    }

    #[test]
    fn normalize_authorize_request_rejects_plain_pkce() {
        let error = normalize_authorize_request(OAuthAuthorizeRequest {
            response_type: Some("code".to_owned()),
            client_id: Some("client".to_owned()),
            redirect_uri: Some("https://client.example/callback".to_owned()),
            code_challenge: Some("challenge".to_owned()),
            code_challenge_method: Some("plain".to_owned()),
            ..OAuthAuthorizeRequest::default()
        })
        .expect_err("plain pkce");

        assert_eq!(error, "unsupported code_challenge_method");
        assert!(!code_challenge_method_is_supported(Some("plain")));
        assert!(code_challenge_method_is_supported(Some("S256")));
    }

    #[test]
    fn authorize_error_redirect_includes_state() {
        let location = build_oauth_authorize_error_redirect_url(
            "https://client.example/callback",
            Some("state-123"),
            "invalid_scope",
            "Requested scope is outside the registered app scopes",
        )
        .expect("redirect url");

        assert!(location.starts_with("https://client.example/callback?"));
        assert!(location.contains("error=invalid_scope"));
        assert!(location.contains("state=state-123"));
        assert!(location.contains("error_description="));
    }

    #[test]
    fn authorize_failure_keeps_html_for_invalid_redirect() {
        let failure = OAuthAuthorizeFailure::Html {
            message: "Redirect URI is not registered for this OAuth client".to_owned(),
        };
        assert!(authorize_failure_should_use_html_page(&failure));

        let redirect_failure = OAuthAuthorizeFailure::Redirect {
            redirect_uri: "https://client.example/callback".to_owned(),
            state: Some("abc".to_owned()),
            error: "invalid_request",
            description: "unsupported code_challenge_method".to_owned(),
        };
        assert!(!authorize_failure_should_use_html_page(&redirect_failure));
    }
}
