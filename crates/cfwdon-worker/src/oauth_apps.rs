use crate::{
    Request, Response, Result, RouteContext, escape_html, find_account_by_email,
    find_account_by_id, find_account_by_username, generate_entity_id, load_config,
    now_unix_timestamp,
};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use pbkdf2::pbkdf2_hmac_array;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use worker::{D1Database, ResponseBody, d1::D1Type};

const AUTHORIZATION_CODE_TTL_SECONDS: i64 = 600;
const PASSWORD_HASH_ALGORITHM: &str = "pbkdf2-sha256";
const PASSWORD_HASH_ITERATIONS: u32 = 210_000;

#[derive(Debug, Default, Deserialize)]
struct CreateAppRequest {
    client_name: Option<String>,
    scopes: Option<String>,
    website: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OAuthTokenRequest {
    grant_type: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    code: Option<String>,
    code_verifier: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct OAuthAuthorizeRequest {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

#[derive(Debug, Default)]
struct OAuthAuthorizeLoginRequest {
    username: Option<String>,
    password: Option<String>,
    authorize: OAuthAuthorizeRequest,
}

#[derive(Debug)]
struct ParsedCreateAppRequest {
    client_name: String,
    website: Option<String>,
    scopes: Vec<String>,
    redirect_uris: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthAppRow {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) website: Option<String>,
    scopes_json: String,
    redirect_uri_legacy: String,
    redirect_uris_json: String,
    client_id: String,
    client_secret: String,
    client_secret_expires_at: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OAuthAccessTokenRow {
    pub(crate) access_token: String,
    pub(crate) oauth_app_id: i64,
    pub(crate) account_id: String,
    scopes_json: String,
}

#[derive(Debug, Deserialize)]
struct OAuthAuthorizationCodeRow {
    code: String,
    oauth_app_id: i64,
    account_id: String,
    redirect_uri: String,
    scopes_json: String,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    expires_at: i64,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_oauth_token_document(access_token: &str, scope: &str) -> serde_json::Value {
    serde_json::json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "scope": scope,
        "created_at": now_unix_timestamp(),
    })
}

pub(crate) fn hash_account_password(password: &str, salt: &str) -> String {
    let digest = pbkdf2_hmac_array::<Sha256, 32>(
        password.as_bytes(),
        salt.as_bytes(),
        PASSWORD_HASH_ITERATIONS,
    );
    format!(
        "{}${}${}${}",
        PASSWORD_HASH_ALGORITHM,
        PASSWORD_HASH_ITERATIONS,
        salt,
        URL_SAFE_NO_PAD.encode(digest)
    )
}

pub(crate) fn verify_account_password_hash(password: &str, hash: &str) -> bool {
    let mut parts = hash.split('$');
    let (Some(algorithm), Some(iterations), Some(salt), Some(expected), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return false;
    };
    let Ok(iterations) = iterations.parse::<u32>() else {
        return false;
    };
    if algorithm != PASSWORD_HASH_ALGORITHM || iterations != PASSWORD_HASH_ITERATIONS {
        return false;
    }
    hash_account_password(password, salt)
        .split('$')
        .next_back()
        .is_some_and(|actual| constant_time_eq(actual.as_bytes(), expected.as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

pub(crate) async fn store_account_password(
    db: &D1Database,
    account_id: &str,
    password: &str,
) -> Result<()> {
    let salt = generate_entity_id(16)?;
    let password_hash = hash_account_password(password, &salt);
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(password_hash.as_str()),
    ];
    db.prepare(
        "INSERT OR REPLACE INTO account_password_credentials (
            account_id,
            password_hash,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn load_account_password_hash(db: &D1Database, account_id: &str) -> Result<Option<String>> {
    #[derive(Debug, Deserialize)]
    struct PasswordHashRow {
        password_hash: String,
    }
    let binding = D1Type::Text(account_id);
    Ok(db
        .prepare(
            "SELECT password_hash
             FROM account_password_credentials
             WHERE account_id = ?1
             LIMIT 1",
        )
        .bind_refs(&binding)?
        .first::<PasswordHashRow>(None)
        .await?
        .map(|row| row.password_hash))
}

pub(crate) fn oauth_app_scopes(row: &OAuthAppRow) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&row.scopes_json).unwrap_or_default()
}

pub(crate) fn oauth_access_token_has_any_scope_json(scopes_json: &str, scopes: &[&str]) -> bool {
    serde_json::from_str::<Vec<String>>(scopes_json)
        .unwrap_or_default()
        .iter()
        .any(|scope| scopes.contains(&scope.as_str()))
}

pub(crate) fn oauth_app_has_any_scope(row: &OAuthAppRow, scopes: &[&str]) -> bool {
    oauth_app_scopes(row)
        .iter()
        .any(|scope| scopes.contains(&scope.as_str()))
}

pub(crate) fn oauth_access_token_has_any_scope(row: &OAuthAccessTokenRow, scopes: &[&str]) -> bool {
    oauth_access_token_has_any_scope_json(&row.scopes_json, scopes)
}

fn oauth_app_redirect_uris(row: &OAuthAppRow) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&row.redirect_uris_json).unwrap_or_default()
}

pub(crate) fn build_app_verify_credentials_document_from_parts(
    id: &str,
    name: &str,
    website: Option<&str>,
    scopes: &[String],
    redirect_uris: &[String],
    redirect_uri: &str,
    vapid_key: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "website": website,
        "scopes": scopes,
        "redirect_uris": redirect_uris,
        "redirect_uri": redirect_uri,
        "vapid_key": vapid_key,
    })
}

pub(crate) fn build_app_verify_credentials_document_from_row(
    row: &OAuthAppRow,
    config: &cfwdon_core::AppConfig,
) -> serde_json::Value {
    let scopes = oauth_app_scopes(row);
    let redirect_uris = oauth_app_redirect_uris(row);
    build_app_verify_credentials_document_from_parts(
        &row.id.to_string(),
        &row.name,
        row.website.as_deref(),
        &scopes,
        &redirect_uris,
        &row.redirect_uri_legacy,
        config.web_push_vapid_public_key.as_deref().unwrap_or(""),
    )
}

fn app_document(row: &OAuthAppRow, config: &cfwdon_core::AppConfig) -> serde_json::Value {
    let scopes = oauth_app_scopes(row);
    let redirect_uris = oauth_app_redirect_uris(row);
    serde_json::json!({
        "id": row.id.to_string(),
        "name": row.name,
        "website": row.website,
        "scopes": scopes,
        "redirect_uri": row.redirect_uri_legacy,
        "redirect_uris": redirect_uris,
        "client_id": row.client_id,
        "client_secret": row.client_secret,
        "client_secret_expires_at": row.client_secret_expires_at,
        "vapid_key": config.web_push_vapid_public_key.as_deref().unwrap_or(""),
    })
}

pub(crate) fn app_bearer_token_from_request(req: &Request) -> Result<Option<String>> {
    let Some(value) = req.headers().get("Authorization")? else {
        return Ok(None);
    };
    Ok(parse_bearer_authorization_header(&value))
}

pub(crate) fn parse_bearer_authorization_header(value: &str) -> Option<String> {
    let value = value.trim();
    let token = value.strip_prefix("Bearer ")?.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

pub(crate) fn parse_basic_authorization_header(value: &str) -> Option<(String, String)> {
    let value = value.trim();
    let encoded = value.strip_prefix("Basic ")?.trim();
    let decoded = STANDARD.decode(encoded).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (client_id, client_secret) = decoded.split_once(':')?;
    let client_id = client_id.trim();
    let client_secret = client_secret.trim();
    if client_id.is_empty() || client_secret.is_empty() {
        return None;
    }
    Some((client_id.to_owned(), client_secret.to_owned()))
}

pub(crate) async fn find_oauth_app_by_bearer_token(
    db: &D1Database,
    token: &str,
) -> Result<Option<OAuthAppRow>> {
    let binding = D1Type::Text(token);
    db.prepare(
        "SELECT id, name, website, scopes_json, redirect_uri_legacy, redirect_uris_json,
                client_id, client_secret, client_secret_expires_at
         FROM oauth_apps
         WHERE client_secret = ?1
            OR client_id = ?1
         ORDER BY id ASC
         LIMIT 1",
    )
    .bind_refs(&[binding])?
    .first::<OAuthAppRow>(None)
    .await
}

pub(crate) async fn find_oauth_app_by_client_id(
    db: &D1Database,
    client_id: &str,
) -> Result<Option<OAuthAppRow>> {
    let client_id_binding = D1Type::Text(client_id);
    db.prepare(
        "SELECT id, name, website, scopes_json, redirect_uri_legacy, redirect_uris_json,
                client_id, client_secret, client_secret_expires_at
         FROM oauth_apps
         WHERE client_id = ?1
         LIMIT 1",
    )
    .bind_refs(&[client_id_binding])?
    .first::<OAuthAppRow>(None)
    .await
}

pub(crate) async fn find_oauth_app_by_id(db: &D1Database, id: i64) -> Result<Option<OAuthAppRow>> {
    let binding = D1Type::Integer(i32::try_from(id).unwrap_or(i32::MAX));
    db.prepare(
        "SELECT id, name, website, scopes_json, redirect_uri_legacy, redirect_uris_json,
                client_id, client_secret, client_secret_expires_at
         FROM oauth_apps
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&[binding])?
    .first::<OAuthAppRow>(None)
    .await
}

pub(crate) async fn find_oauth_app_id_by_bearer_token(
    db: &D1Database,
    token: &str,
) -> Result<Option<i64>> {
    Ok(find_oauth_app_by_bearer_token(db, token)
        .await?
        .map(|row| row.id))
}

pub(crate) async fn find_oauth_access_token_by_bearer_token(
    db: &D1Database,
    token: &str,
) -> Result<Option<OAuthAccessTokenRow>> {
    let binding = D1Type::Text(token);
    db.prepare(
        "SELECT access_token, oauth_app_id, account_id, scopes_json
         FROM oauth_access_tokens
         WHERE access_token = ?1
         LIMIT 1",
    )
    .bind_refs(&[binding])?
    .first::<OAuthAccessTokenRow>(None)
    .await
}

pub(crate) async fn issue_oauth_access_token(
    db: &D1Database,
    oauth_app_id: i64,
    account_id: &str,
    scopes: &[String],
) -> Result<OAuthAccessTokenRow> {
    let access_token = generate_entity_id(32)?;
    let scopes_json = serde_json::to_string(scopes).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize access token scopes: {error}"))
    })?;
    let bindings = [
        D1Type::Text(access_token.as_str()),
        D1Type::Integer(i32::try_from(oauth_app_id).unwrap_or(i32::MAX)),
        D1Type::Text(account_id),
        D1Type::Text(scopes_json.as_str()),
    ];
    db.prepare(
        "INSERT INTO oauth_access_tokens (
            access_token,
            oauth_app_id,
            account_id,
            scopes_json,
            created_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(OAuthAccessTokenRow {
        access_token,
        oauth_app_id,
        account_id: account_id.to_owned(),
        scopes_json,
    })
}

async fn issue_oauth_authorization_code(
    db: &D1Database,
    oauth_app_id: i64,
    account_id: &str,
    redirect_uri: &str,
    scopes: &[String],
    code_challenge: Option<&str>,
    code_challenge_method: Option<&str>,
) -> Result<String> {
    let code = generate_entity_id(32)?;
    let scopes_json = serde_json::to_string(scopes).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize authorization scopes: {error}"))
    })?;
    let expires_at = now_unix_timestamp() + AUTHORIZATION_CODE_TTL_SECONDS;
    let bindings = [
        D1Type::Text(code.as_str()),
        D1Type::Integer(i32::try_from(oauth_app_id).unwrap_or(i32::MAX)),
        D1Type::Text(account_id),
        D1Type::Text(redirect_uri),
        D1Type::Text(scopes_json.as_str()),
        code_challenge.map(D1Type::Text).unwrap_or(D1Type::Null),
        code_challenge_method
            .map(D1Type::Text)
            .unwrap_or(D1Type::Null),
        D1Type::Integer(i32::try_from(expires_at).unwrap_or(i32::MAX)),
    ];
    db.prepare(
        "INSERT INTO oauth_authorization_codes (
            code,
            oauth_app_id,
            account_id,
            redirect_uri,
            scopes_json,
            code_challenge,
            code_challenge_method,
            expires_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(code)
}

async fn load_oauth_authorization_code(
    db: &D1Database,
    code: &str,
) -> Result<Option<OAuthAuthorizationCodeRow>> {
    let binding = D1Type::Text(code);
    db.prepare(
        "SELECT code, oauth_app_id, account_id, redirect_uri, scopes_json,
                code_challenge, code_challenge_method, expires_at
         FROM oauth_authorization_codes
         WHERE code = ?1
         LIMIT 1",
    )
    .bind_refs(&binding)?
    .first::<OAuthAuthorizationCodeRow>(None)
    .await
}

async fn delete_oauth_authorization_code(db: &D1Database, code: &str) -> Result<()> {
    let binding = D1Type::Text(code);
    db.prepare("DELETE FROM oauth_authorization_codes WHERE code = ?1")
        .bind_refs(&binding)?
        .run()
        .await?;
    Ok(())
}

fn normalize_required_client_name(value: Option<String>) -> std::result::Result<String, String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Validation failed: Name can't be blank".to_owned())
}

fn normalize_scopes(value: Option<String>) -> Vec<String> {
    let raw = value.unwrap_or_else(|| "read".to_owned());
    let mut scopes = Vec::new();
    for scope in raw.split_whitespace() {
        let normalized = scope.trim().to_owned();
        if !normalized.is_empty() && !scopes.contains(&normalized) {
            scopes.push(normalized);
        }
    }
    if scopes.is_empty() {
        vec!["read".to_owned()]
    } else {
        scopes
    }
}

fn normalize_website(value: Option<String>) -> std::result::Result<Option<String>, String> {
    let Some(value) = value.map(|value| value.trim().to_owned()) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let url = Url::parse(&value)
        .map_err(|_| "Validation failed: Website must be a valid URL".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Validation failed: Website must be a valid URL".to_owned());
    }
    Ok(Some(value))
}

fn validate_redirect_uri(value: &str) -> std::result::Result<String, String> {
    if value == "urn:ietf:wg:oauth:2.0:oob" {
        return Ok(value.to_owned());
    }
    let url = Url::parse(value)
        .map_err(|_| "Validation failed: Redirect URI must be an absolute URI.".to_owned())?;
    if url.host_str().is_none() {
        return Err("Validation failed: Redirect URI must be an absolute URI.".to_owned());
    }
    Ok(value.to_owned())
}

fn normalize_redirect_uris(values: Vec<String>) -> std::result::Result<Vec<String>, String> {
    let mut redirect_uris = Vec::new();
    for value in values {
        for candidate in value.lines() {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            let normalized = validate_redirect_uri(candidate)?;
            if !redirect_uris.contains(&normalized) {
                redirect_uris.push(normalized);
            }
        }
    }
    if redirect_uris.is_empty() {
        return Err("Validation failed: Redirect URI must be an absolute URI.".to_owned());
    }
    Ok(redirect_uris)
}

fn authorization_redirect_with_params(
    redirect_uri: &str,
    params: &[(&str, String)],
) -> Result<Response> {
    if redirect_uri == "urn:ietf:wg:oauth:2.0:oob" {
        let code = params
            .iter()
            .find_map(|(name, value)| (*name == "code").then_some(value.as_str()))
            .unwrap_or_default();
        return html_response(
            &format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>Authorization code</title></head><body><main><h1>Authorization code</h1><p><code>{}</code></p></main></body></html>",
                escape_html(code)
            ),
            200,
        );
    }

    let mut url = Url::parse(redirect_uri)
        .map_err(|error| worker::Error::RustError(format!("invalid redirect URI: {error}")))?;
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in params {
            if !value.is_empty() {
                query.append_pair(name, value);
            }
        }
    }
    let mut response = Response::empty()?.with_status(302);
    response.headers_mut().set("Location", url.as_str())?;
    Ok(response)
}

fn html_response(body: &str, status: u16) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(body.as_bytes().to_vec()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response.with_status(status))
}

fn oauth_login_page(
    request: &OAuthAuthorizeRequest,
    app: &OAuthAppRow,
    error: Option<&str>,
) -> Result<Response> {
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
    let error_html = error
        .map(|message| format!("<p role=\"alert\">{}</p>", escape_html(message)))
        .unwrap_or_default();
    html_response(
        &format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Authorize {app}</title></head><body><main><h1>Authorize {app}</h1>{error}<form method=\"post\" action=\"/oauth/authorize\">{hidden}<label>Username or email <input name=\"username\" autocomplete=\"username\" required></label><label>Password <input name=\"password\" type=\"password\" autocomplete=\"current-password\" required></label><button type=\"submit\">Authorize</button></form></main></body></html>",
            app = escape_html(&app.name),
            error = error_html,
            hidden = hidden,
        ),
        error.map(|_| 401).unwrap_or(200),
    )
}

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
    if request.code_challenge.is_some()
        && !matches!(
            code_challenge_method.as_deref(),
            Some("S256") | Some("plain")
        )
    {
        return Err("unsupported code_challenge_method".to_owned());
    }
    Ok(OAuthAuthorizeRequest {
        response_type: Some(response_type),
        client_id: Some(client_id),
        redirect_uri: Some(redirect_uri),
        scope: request.scope.map(|value| value.trim().to_owned()),
        state: request.state.map(|value| value.trim().to_owned()),
        code_challenge: request
            .code_challenge
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        code_challenge_method,
    })
}

async fn validate_authorize_request(
    db: &D1Database,
    request: OAuthAuthorizeRequest,
) -> std::result::Result<(OAuthAuthorizeRequest, OAuthAppRow, Vec<String>), String> {
    let request = normalize_authorize_request(request)?;
    let client_id = request.client_id.as_deref().expect("normalized client_id");
    let Some(app) = find_oauth_app_by_client_id(db, client_id)
        .await
        .map_err(|error| format!("failed to load OAuth app: {error}"))?
    else {
        return Err("Unknown OAuth client".to_owned());
    };
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .expect("normalized redirect_uri");
    if !oauth_app_redirect_uris(&app)
        .iter()
        .any(|value| value == redirect_uri)
    {
        return Err("Redirect URI is not registered for this OAuth client".to_owned());
    }
    let requested_scopes = requested_oauth_token_scopes(request.scope.clone());
    let registered_scopes = oauth_app_scopes(&app);
    if requested_scopes
        .iter()
        .any(|scope| !registered_scopes.contains(scope))
    {
        return Err("Requested scope is outside the registered app scopes".to_owned());
    }
    Ok((request, app, requested_scopes))
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

async fn parse_create_app_request(
    req: &mut Request,
) -> std::result::Result<ParsedCreateAppRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (request, redirect_uris) = if content_type.contains("application/json") {
        let payload = req
            .json::<serde_json::Value>()
            .await
            .map_err(|error| format!("invalid JSON app payload: {error}"))?;
        let redirect_uris = match payload.get("redirect_uris") {
            Some(serde_json::Value::String(value)) => vec![value.clone()],
            Some(serde_json::Value::Array(values)) => values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
            _ => payload
                .get("redirect_uri")
                .and_then(serde_json::Value::as_str)
                .map(|value| vec![value.to_owned()])
                .unwrap_or_default(),
        };
        (
            serde_json::from_value::<CreateAppRequest>(payload)
                .map_err(|error| format!("invalid JSON app payload: {error}"))?,
            redirect_uris,
        )
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form app payload: {error}"))?;
        let redirect_uris = form
            .get_all("redirect_uris[]")
            .map(|entries| {
                entries
                    .into_iter()
                    .filter_map(|entry| match entry {
                        worker::FormEntry::Field(value) => Some(value),
                        worker::FormEntry::File(_) => None,
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|values| !values.is_empty())
            .unwrap_or_else(|| {
                form.get_field("redirect_uris")
                    .map(|value| vec![value])
                    .unwrap_or_default()
            });
        (
            CreateAppRequest {
                client_name: form.get_field("client_name"),
                scopes: form.get_field("scopes"),
                website: form.get_field("website"),
            },
            redirect_uris,
        )
    };

    Ok(ParsedCreateAppRequest {
        client_name: normalize_required_client_name(request.client_name)?,
        website: normalize_website(request.website)?,
        scopes: normalize_scopes(request.scopes),
        redirect_uris: normalize_redirect_uris(redirect_uris)?,
    })
}

async fn insert_oauth_app(
    db: &worker::D1Database,
    request: &ParsedCreateAppRequest,
) -> Result<OAuthAppRow> {
    let scopes_json = serde_json::to_string(&request.scopes).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize app scopes: {error}"))
    })?;
    let redirect_uris_json = serde_json::to_string(&request.redirect_uris).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize app redirect URIs: {error}"))
    })?;
    let redirect_uri_legacy = request.redirect_uris.join("\n");
    let client_id = generate_entity_id(24)?;
    let client_secret = generate_entity_id(32)?;
    let bindings = [
        D1Type::Text(request.client_name.as_str()),
        request
            .website
            .as_deref()
            .map(D1Type::Text)
            .unwrap_or(D1Type::Null),
        D1Type::Text(scopes_json.as_str()),
        D1Type::Text(redirect_uris_json.as_str()),
        D1Type::Text(redirect_uri_legacy.as_str()),
        D1Type::Text(client_id.as_str()),
        D1Type::Text(client_secret.as_str()),
    ];
    db.prepare(
        "INSERT INTO oauth_apps (
            name,
            website,
            scopes_json,
            redirect_uris_json,
            redirect_uri_legacy,
            client_id,
            client_secret,
            client_secret_expires_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            0
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let client_id_binding = D1Type::Text(client_id.as_str());
    db.prepare(
        "SELECT id, name, website, scopes_json, redirect_uri_legacy, redirect_uris_json,
                client_id, client_secret, client_secret_expires_at
         FROM oauth_apps
         WHERE client_id = ?1
         LIMIT 1",
    )
    .bind_refs(&client_id_binding)?
    .first::<OAuthAppRow>(None)
    .await?
    .ok_or_else(|| worker::Error::RustError("created app could not be reloaded".to_owned()))
}

fn pkce_code_challenge(verifier: &str, method: Option<&str>) -> String {
    match method {
        Some("S256") => URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        _ => verifier.to_owned(),
    }
}

fn pkce_verifier_matches(verifier: &str, challenge: &str, method: Option<&str>) -> bool {
    constant_time_eq(
        pkce_code_challenge(verifier, method).as_bytes(),
        challenge.as_bytes(),
    )
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
    let Some(password_hash) = load_account_password_hash(db, &account.id).await? else {
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
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .expect("validated redirect_uri");
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
    let db = ctx.d1(&config.database_binding)?;
    if req.method().as_ref() == "POST" {
        let login = match parse_oauth_authorize_login_request(&mut req).await {
            Ok(login) => login,
            Err(message) => return Response::error(&message, 422),
        };
        let (authorize, app, scopes) = match validate_authorize_request(&db, login.authorize).await
        {
            Ok(value) => value,
            Err(message) => return Response::error(&message, 400),
        };
        let Some(account) =
            authorize_account_by_password(&db, login.username, login.password).await?
        else {
            return oauth_login_page(&authorize, &app, Some("Invalid username or password."));
        };
        return redirect_with_authorization_code(&db, &authorize, &app, &account.id, &scopes).await;
    }

    let authorize = match req.query::<OAuthAuthorizeRequest>() {
        Ok(query) => query,
        Err(_) => return Response::error("Invalid authorization request", 400),
    };
    let (authorize, app, scopes) = match validate_authorize_request(&db, authorize).await {
        Ok(value) => value,
        Err(message) => return Response::error(&message, 400),
    };
    if let Some(account) = crate::find_authenticated_local_account(&req, &db, &config).await? {
        return redirect_with_authorization_code(&db, &authorize, &app, &account.id, &scopes).await;
    }
    oauth_login_page(&authorize, &app, None)
}

fn oauth_invalid_client_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "invalid_client",
        "error_description": "Client authentication failed due to unknown client, no client authentication included, or unsupported authentication method.",
    }))?
    .with_status(401))
}

fn oauth_invalid_scope_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "invalid_scope",
        "error_description": "The requested scope is invalid, unknown, or malformed.",
    }))?
    .with_status(400))
}

fn oauth_unsupported_grant_type_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "unsupported_grant_type",
        "error_description": "The authorization grant type is not supported by the authorization server.",
    }))?
    .with_status(400))
}

async fn oauth_authorization_code_token_response(
    db: &D1Database,
    request: OAuthTokenRequest,
    header_credentials: Option<(String, String)>,
) -> Result<Response> {
    let client_id = header_credentials
        .as_ref()
        .map(|(client_id, _)| client_id.clone())
        .or_else(|| {
            request
                .client_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let client_secret = header_credentials
        .as_ref()
        .map(|(_, client_secret)| client_secret.clone())
        .or_else(|| {
            request
                .client_secret
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let code = request
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (Some(client_id), Some(code), Some(redirect_uri)) = (client_id, code, redirect_uri) else {
        return oauth_invalid_client_response();
    };
    let Some(app) = find_oauth_app_by_client_id(db, &client_id).await? else {
        return oauth_invalid_client_response();
    };
    if let Some(client_secret) = client_secret.as_ref()
        && app.client_secret != *client_secret
    {
        return oauth_invalid_client_response();
    }
    let Some(code_row) = load_oauth_authorization_code(db, code).await? else {
        return oauth_invalid_client_response();
    };
    if code_row.oauth_app_id != app.id || code_row.redirect_uri != redirect_uri {
        return oauth_invalid_client_response();
    }
    if code_row.expires_at < now_unix_timestamp() {
        delete_oauth_authorization_code(db, &code_row.code).await?;
        return oauth_invalid_client_response();
    }
    if let Some(challenge) = code_row.code_challenge.as_deref() {
        let Some(verifier) = request
            .code_verifier
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return oauth_invalid_client_response();
        };
        if !pkce_verifier_matches(
            verifier,
            challenge,
            code_row.code_challenge_method.as_deref(),
        ) {
            return oauth_invalid_client_response();
        }
    } else if client_secret.is_none() {
        return oauth_invalid_client_response();
    }

    let scopes = serde_json::from_str::<Vec<String>>(&code_row.scopes_json).unwrap_or_default();
    let access_token = issue_oauth_access_token(db, app.id, &code_row.account_id, &scopes).await?;
    crate::link_oauth_app_to_account(db, app.id, &code_row.account_id).await?;
    delete_oauth_authorization_code(db, &code_row.code).await?;
    if find_account_by_id(db, &code_row.account_id)
        .await?
        .is_none()
    {
        return oauth_invalid_client_response();
    }
    Response::from_json(&build_oauth_token_document(
        &access_token.access_token,
        &scopes.join(" "),
    ))
}

async fn parse_oauth_token_request(
    req: &mut Request,
) -> std::result::Result<OAuthTokenRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        req.json::<OAuthTokenRequest>()
            .await
            .map_err(|error| format!("invalid JSON token payload: {error}"))
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form token payload: {error}"))?;
        Ok(OAuthTokenRequest {
            grant_type: form.get_field("grant_type"),
            client_id: form.get_field("client_id"),
            client_secret: form.get_field("client_secret"),
            redirect_uri: form.get_field("redirect_uri"),
            scope: form.get_field("scope"),
            code: form.get_field("code"),
            code_verifier: form.get_field("code_verifier"),
        })
    }
}

fn requested_oauth_token_scopes(value: Option<String>) -> Vec<String> {
    let raw = value.unwrap_or_else(|| "read".to_owned());
    let mut scopes = Vec::new();
    for scope in raw.split_whitespace() {
        let normalized = scope.trim().to_owned();
        if !normalized.is_empty() && !scopes.contains(&normalized) {
            scopes.push(normalized);
        }
    }
    if scopes.is_empty() {
        vec!["read".to_owned()]
    } else {
        scopes
    }
}

pub(crate) async fn create_app_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let request = match parse_create_app_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(&message, 422),
    };
    let app = insert_oauth_app(&db, &request).await?;
    Response::from_json(&app_document(&app, &config))
}

pub(crate) async fn oauth_token_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let request = match parse_oauth_token_request(&mut req).await {
        Ok(request) => request,
        Err(_) => return oauth_invalid_client_response(),
    };
    let grant_type = request
        .grant_type
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    let header_credentials = req
        .headers()
        .get("Authorization")?
        .as_deref()
        .and_then(parse_basic_authorization_header);
    let db = ctx.d1(&load_config(&ctx).database_binding)?;
    if grant_type == "authorization_code" {
        return oauth_authorization_code_token_response(&db, request, header_credentials).await;
    }
    if grant_type != "client_credentials" {
        return oauth_unsupported_grant_type_response();
    }
    let client_id = header_credentials
        .as_ref()
        .map(|(client_id, _)| client_id.clone())
        .or_else(|| {
            request
                .client_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let client_secret = header_credentials
        .as_ref()
        .map(|(_, client_secret)| client_secret.clone())
        .or_else(|| {
            request
                .client_secret
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (Some(client_id), Some(client_secret), Some(redirect_uri)) =
        (client_id, client_secret, redirect_uri)
    else {
        return oauth_invalid_client_response();
    };

    let Some(app) = find_oauth_app_by_client_id(&db, &client_id).await? else {
        return oauth_invalid_client_response();
    };
    if app.client_secret != client_secret {
        return oauth_invalid_client_response();
    }
    if !oauth_app_redirect_uris(&app)
        .iter()
        .any(|value| value == redirect_uri)
    {
        return oauth_invalid_client_response();
    }

    let requested_scopes = requested_oauth_token_scopes(request.scope);
    let registered_scopes = oauth_app_scopes(&app);
    if requested_scopes
        .iter()
        .any(|scope| !registered_scopes.contains(scope))
    {
        return oauth_invalid_scope_response();
    }

    Response::from_json(&build_oauth_token_document(
        &app.client_secret,
        &requested_scopes.join(" "),
    ))
}
