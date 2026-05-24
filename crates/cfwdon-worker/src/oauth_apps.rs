use crate::auth::find_account_by_email;
use crate::auth::{find_account_by_id, find_account_by_username};
use crate::id_utils::generate_entity_id;
use crate::runtime_config::load_config;
use crate::time_html::{escape_html, now_unix_timestamp};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use cfwdon_domain::LocalAccount;
use pbkdf2::pbkdf2_hmac_array;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use worker::{D1Database, FormData, ResponseBody, d1::D1Type};
use worker::{Request, Response, Result, RouteContext};

const AUTHORIZATION_CODE_TTL_SECONDS: i64 = 600;
const PASSWORD_HASH_ALGORITHM: &str = "pbkdf2-sha256";
const PASSWORD_HASH_ITERATIONS: u32 = 210_000;
const FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL: &str =
    "SELECT id, name, website, scopes_json, redirect_uri_legacy, redirect_uris_json,
                client_id, client_secret, client_secret_expires_at
         FROM oauth_apps
         WHERE client_secret = ?1
         ORDER BY id ASC
         LIMIT 1";

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
pub(crate) struct OAuthAuthorizeRequest {
    pub(crate) response_type: Option<String>,
    pub(crate) client_id: Option<String>,
    pub(crate) redirect_uri: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) code_challenge: Option<String>,
    pub(crate) code_challenge_method: Option<String>,
}

#[derive(Debug, Default)]
struct OAuthAuthorizeLoginRequest {
    username: Option<String>,
    password: Option<String>,
    approve: bool,
    authorize: OAuthAuthorizeRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccessAuthorizeGetAction {
    RedirectToLogin,
    MissingLinkedAccount,
    ShowConsent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccessAuthorizePostAction {
    RedirectToLogin,
    ShowConsent,
    IssueCode,
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
    scopes_json: String,
}

#[derive(Clone, Debug)]
pub(crate) struct OAuthAccessTokenWithAccount {
    pub(crate) token: OAuthAccessTokenRow,
    pub(crate) account: Option<LocalAccount>,
}

#[derive(Clone, Debug, Deserialize)]
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
    let redirect_uris = serde_json::from_str::<Vec<String>>(&row.redirect_uris_json)
        .unwrap_or_default()
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();
    if !redirect_uris.is_empty() {
        return redirect_uris;
    }
    row.redirect_uri_legacy
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn redirect_uri_matches_registered(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }
    let (Ok(registered_url), Ok(requested_url)) = (Url::parse(registered), Url::parse(requested))
    else {
        return false;
    };
    if registered_url.scheme() != requested_url.scheme()
        || registered_url.username() != requested_url.username()
        || registered_url.password() != requested_url.password()
        || registered_url.host_str() != requested_url.host_str()
        || registered_url.port_or_known_default() != requested_url.port_or_known_default()
        || registered_url.query() != requested_url.query()
        || registered_url.fragment() != requested_url.fragment()
    {
        return false;
    }
    if matches!(registered_url.scheme(), "http" | "https") {
        return registered_url.path() == requested_url.path();
    }
    matches!(
        (registered_url.path(), requested_url.path()),
        ("", "/") | ("/", "")
    )
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
    db.prepare(FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL)
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

pub(crate) async fn find_oauth_apps_by_ids(
    db: &D1Database,
    ids: &[i64],
) -> Result<Vec<OAuthAppRow>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = crate::sql_placeholders(1, ids.len());
    let sql = format!(
        "SELECT id, name, website, scopes_json, redirect_uri_legacy, redirect_uris_json,
                client_id, client_secret, client_secret_expires_at
         FROM oauth_apps
         WHERE id IN ({placeholders})"
    );
    let bindings = ids
        .iter()
        .map(|id| D1Type::Integer(i32::try_from(*id).unwrap_or(i32::MAX)))
        .collect::<Vec<_>>();

    db.prepare(&sql)
        .bind_refs(bindings.iter())?
        .all()
        .await?
        .results::<OAuthAppRow>()
}

pub(crate) async fn find_oauth_app_id_by_bearer_token(
    db: &D1Database,
    token: &str,
) -> Result<Option<i64>> {
    Ok(find_oauth_app_by_bearer_token(db, token)
        .await?
        .map(|row| row.id))
}

pub(crate) async fn find_oauth_access_token_with_account_by_bearer_token(
    db: &D1Database,
    token: &str,
) -> Result<Option<OAuthAccessTokenWithAccount>> {
    let binding = D1Type::Text(token);
    let Some(row) = db
        .prepare(
            "SELECT t.access_token,
                    t.oauth_app_id,
                    t.scopes_json,
                    a.id,
                    a.username,
                    a.access_email,
                    a.display_name,
                    a.bio_html,
                    a.bio_text,
                    a.fields_json,
                    a.locked,
                    a.bot,
                    a.discoverable,
                    a.default_post_visibility,
                    a.default_quote_policy,
                    a.default_sensitive,
                    a.default_language,
                    a.avatar_object_key,
                    a.avatar_content_type,
                    a.header_object_key,
                    a.header_content_type,
                    a.private_key_jwk,
                    a.public_key_pem,
                    a.created_at
             FROM oauth_access_tokens t
             LEFT JOIN accounts a ON a.id = t.account_id
             WHERE t.access_token = ?1
             LIMIT 1",
        )
        .bind_refs(&[binding])?
        .first::<serde_json::Value>(None)
        .await?
    else {
        return Ok(None);
    };

    let token = serde_json::from_value::<OAuthAccessTokenRow>(row.clone()).map_err(|error| {
        worker::Error::RustError(format!("failed to decode OAuth access token row: {error}"))
    })?;
    let account = match row.get("id").and_then(serde_json::Value::as_str) {
        Some(_) => Some(
            serde_json::from_value::<crate::AccountRow>(row)
                .map(LocalAccount::from)
                .map_err(|error| {
                    worker::Error::RustError(format!(
                        "failed to decode OAuth access token account row: {error}"
                    ))
                })?,
        ),
        None => None,
    };

    Ok(Some(OAuthAccessTokenWithAccount { token, account }))
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
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_none() {
        return Err("Validation failed: Redirect URI must be an absolute URI.".to_owned());
    }
    Ok(value.to_owned())
}

fn normalize_redirect_uris(values: Vec<String>) -> std::result::Result<Vec<String>, String> {
    let mut redirect_uris = Vec::new();
    for value in values {
        for candidate in value.split_whitespace() {
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
    redirect_response(url.as_str())
}

pub(crate) fn access_login_configured(config: &cfwdon_core::AppConfig) -> bool {
    !config.access_team_domain.trim().is_empty() && !config.access_audience.trim().is_empty()
}

pub(crate) fn cloudflare_access_login_url(
    config: &cfwdon_core::AppConfig,
    redirect_url: &Url,
) -> std::result::Result<Url, String> {
    let team_url = cloudflare_access_team_url(config)?;
    let hostname = redirect_url
        .host_str()
        .ok_or_else(|| "OAuth authorize redirect URL did not include a host".to_owned())?;
    let mut login_url = team_url
        .join(&format!("/cdn-cgi/access/login/{hostname}"))
        .map_err(|error| format!("failed to build Cloudflare Access login URL: {error}"))?;
    let redirect_path = match redirect_url.query() {
        Some(query) => format!("{}?{query}", redirect_url.path()),
        None => redirect_url.path().to_owned(),
    };
    login_url
        .query_pairs_mut()
        .append_pair("kid", config.access_audience.trim())
        .append_pair("redirect_url", &redirect_path);
    Ok(login_url)
}

pub(crate) fn cloudflare_access_logout_url(config: &cfwdon_core::AppConfig) -> String {
    format!(
        "{}/cdn-cgi/access/logout",
        crate::instance_base_url(config).trim_end_matches('/')
    )
}

pub(crate) fn cloudflare_access_team_logout_url(
    config: &cfwdon_core::AppConfig,
) -> std::result::Result<Url, String> {
    cloudflare_access_team_url(config)?
        .join("/cdn-cgi/access/logout")
        .map_err(|error| format!("failed to build Cloudflare Access logout URL: {error}"))
}

fn cloudflare_access_team_url(config: &cfwdon_core::AppConfig) -> std::result::Result<Url, String> {
    let mut team_domain = config.access_team_domain.trim().to_owned();
    if !team_domain.starts_with("http://") && !team_domain.starts_with("https://") {
        team_domain = format!("https://{team_domain}");
    }
    Url::parse(team_domain.trim_end_matches('/'))
        .map_err(|error| format!("invalid Cloudflare Access team domain: {error}"))
}

pub(crate) fn oauth_authorize_url_from_form(
    base_url: &Url,
    request: &OAuthAuthorizeRequest,
) -> std::result::Result<Url, String> {
    let mut authorize_url = base_url.clone();
    authorize_url.set_path("/oauth/authorize");
    authorize_url.set_query(None);
    {
        let mut query = authorize_url.query_pairs_mut();
        for (name, value) in [
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
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                query.append_pair(name, value);
            }
        }
    }
    Ok(authorize_url)
}

fn access_login_redirect_from_authorize_request(
    config: &cfwdon_core::AppConfig,
    base_url: &Url,
    authorize: &OAuthAuthorizeRequest,
) -> Result<Response> {
    let authorize_url =
        oauth_authorize_url_from_form(base_url, authorize).map_err(worker::Error::RustError)?;
    let login_url =
        cloudflare_access_login_url(config, &authorize_url).map_err(worker::Error::RustError)?;
    redirect_response(login_url.as_str())
}

fn redirect_response(location: &str) -> Result<Response> {
    let body = redirect_fallback_body(location);
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?.with_status(302);
    response.headers_mut().set("Location", location)?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(response)
}

fn redirect_fallback_body(location: &str) -> String {
    let escaped = escape_html(location);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url={escaped}\"><title>Redirecting</title></head><body><main><p>Redirecting to <a href=\"{escaped}\">{escaped}</a>.</p></main></body></html>"
    )
}

fn access_authenticated_without_account_response(
    config: &cfwdon_core::AppConfig,
) -> Result<Response> {
    let team_logout_url = cloudflare_access_team_logout_url(config)
        .map(|url| url.to_string())
        .unwrap_or_default();
    Ok(Response::from_json(&serde_json::json!({
        "error": "Cloudflare Access authentication succeeded, but no local account is registered for this email.",
        "registration_url": format!("{}/auth/sign_up", crate::instance_base_url(config)),
        "logout_url": cloudflare_access_logout_url(config),
        "team_logout_url": team_logout_url,
    }))?
    .with_status(403))
}

fn html_response(body: &str, status: u16) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(body.as_bytes().to_vec()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response.with_status(status))
}

fn oauth_authorize_error_response(message: &str, status: u16) -> Result<Response> {
    html_response(
        &format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Authorization error</title></head><body><main><h1>Authorization error</h1><p>{}</p></main></body></html>",
            escape_html(message)
        ),
        status,
    )
}

fn access_authorize_get_action(
    has_authenticated_local_account: bool,
    has_authenticated_access_user_without_account: bool,
) -> AccessAuthorizeGetAction {
    if has_authenticated_local_account {
        AccessAuthorizeGetAction::ShowConsent
    } else if has_authenticated_access_user_without_account {
        AccessAuthorizeGetAction::MissingLinkedAccount
    } else {
        AccessAuthorizeGetAction::RedirectToLogin
    }
}

fn access_authorize_post_action(
    has_authenticated_local_account: bool,
    approved: bool,
    has_valid_credentials: bool,
) -> AccessAuthorizePostAction {
    if !has_authenticated_local_account {
        AccessAuthorizePostAction::RedirectToLogin
    } else if approved || has_valid_credentials {
        AccessAuthorizePostAction::IssueCode
    } else {
        AccessAuthorizePostAction::ShowConsent
    }
}

fn oauth_authorize_page_body(
    request: &OAuthAuthorizeRequest,
    app: &OAuthAppRow,
    error: Option<&str>,
    require_login_credentials: bool,
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
    let error_html = error
        .map(|message| format!("<p role=\"alert\">{}</p>", escape_html(message)))
        .unwrap_or_default();
    let form_body = if require_login_credentials {
        format!(
            "{hidden}<label>Username or email <input name=\"username\" autocomplete=\"username\" required></label><label>Password <input name=\"password\" type=\"password\" autocomplete=\"current-password\" required></label><button type=\"submit\">Authorize</button>"
        )
    } else {
        format!(
            "<p>Approve access for this application.</p>{hidden}<input type=\"hidden\" name=\"approve\" value=\"true\"><button type=\"submit\">Authorize</button>"
        )
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Authorize {app}</title></head><body><main><h1>Authorize {app}</h1>{error}<form method=\"post\" action=\"/oauth/authorize\">{form_body}</form></main></body></html>",
        app = escape_html(&app.name),
        error = error_html,
        form_body = form_body,
    )
}

fn oauth_login_page(
    request: &OAuthAuthorizeRequest,
    app: &OAuthAppRow,
    error: Option<&str>,
) -> Result<Response> {
    html_response(
        &oauth_authorize_page_body(request, app, error, true),
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
        .any(|value| redirect_uri_matches_registered(value, redirect_uri))
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
        approve: form
            .get_field("approve")
            .map(|value| value.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false),
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
    let content_type = request_content_type(req)?;
    let (request, redirect_uris) = if request_is_json(&content_type) {
        create_app_request_from_json_payload(
            req.json::<serde_json::Value>()
                .await
                .map_err(|error| format!("invalid JSON app payload: {error}"))?,
        )?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form app payload: {error}"))?;
        create_app_request_from_form(&form)
    };

    parsed_create_app_request(request, redirect_uris)
}

fn request_content_type(req: &Request) -> std::result::Result<String, String> {
    Ok(req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase())
}

fn request_is_json(content_type: &str) -> bool {
    content_type.contains("application/json")
}

fn create_app_request_from_json_payload(
    payload: serde_json::Value,
) -> std::result::Result<(CreateAppRequest, Vec<String>), String> {
    let redirect_uris = redirect_uris_from_json_payload(&payload);
    let request = serde_json::from_value::<CreateAppRequest>(payload)
        .map_err(|error| format!("invalid JSON app payload: {error}"))?;
    Ok((request, redirect_uris))
}

fn redirect_uris_from_json_payload(payload: &serde_json::Value) -> Vec<String> {
    match payload.get("redirect_uris") {
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
    }
}

fn create_app_request_from_form(form: &FormData) -> (CreateAppRequest, Vec<String>) {
    (
        CreateAppRequest {
            client_name: form.get_field("client_name"),
            scopes: form.get_field("scopes"),
            website: form.get_field("website"),
        },
        redirect_uris_from_form(form),
    )
}

fn redirect_uris_from_form(form: &FormData) -> Vec<String> {
    form.get_all("redirect_uris[]")
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
                .or_else(|| form.get_field("redirect_uri"))
                .map(|value| vec![value])
                .unwrap_or_default()
        })
}

fn parsed_create_app_request(
    request: CreateAppRequest,
    redirect_uris: Vec<String>,
) -> std::result::Result<ParsedCreateAppRequest, String> {
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
        if access_login_configured(&config) {
            let base_url = req.url()?;
            let authenticated_account =
                crate::find_authenticated_local_account(&req, &db, &config).await?;
            let password_account =
                authorize_account_by_password(&db, login.username, login.password).await?;
            return match access_authorize_post_action(
                authenticated_account.is_some(),
                login.approve,
                password_account.is_some(),
            ) {
                AccessAuthorizePostAction::RedirectToLogin => {
                    access_login_redirect_from_authorize_request(&config, &base_url, &authorize)
                }
                AccessAuthorizePostAction::ShowConsent => html_response(
                    &oauth_authorize_page_body(&authorize, &app, None, false),
                    200,
                ),
                AccessAuthorizePostAction::IssueCode => {
                    let account = password_account
                        .or(authenticated_account)
                        .expect("account must exist when issuing authorization code");
                    redirect_with_authorization_code(&db, &authorize, &app, &account.id, &scopes)
                        .await
                }
            };
        }
        let Some(account) =
            authorize_account_by_password(&db, login.username, login.password).await?
        else {
            return oauth_login_page(&authorize, &app, Some("Invalid username or password."));
        };
        return redirect_with_authorization_code(&db, &authorize, &app, &account.id, &scopes).await;
    }

    let authorize = match req.query::<OAuthAuthorizeRequest>() {
        Ok(query) => query,
        Err(_) => return oauth_authorize_error_response("Invalid authorization request", 400),
    };
    let (authorize, app, scopes) = match validate_authorize_request(&db, authorize).await {
        Ok(value) => value,
        Err(message) => return oauth_authorize_error_response(&message, 400),
    };
    let authenticated_account = crate::find_authenticated_local_account(&req, &db, &config).await?;
    if access_login_configured(&config) {
        let action = access_authorize_get_action(
            authenticated_account.is_some(),
            crate::extract_authenticated_user(&req, &config)
                .await?
                .is_some(),
        );
        return match action {
            AccessAuthorizeGetAction::RedirectToLogin => {
                access_login_redirect_from_authorize_request(&config, &req.url()?, &authorize)
            }
            AccessAuthorizeGetAction::MissingLinkedAccount => {
                access_authenticated_without_account_response(&config)
            }
            AccessAuthorizeGetAction::ShowConsent => html_response(
                &oauth_authorize_page_body(&authorize, &app, None, false),
                200,
            ),
        };
    }
    if let Some(account) = authenticated_account {
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

#[derive(Clone, Debug)]
struct OAuthAuthorizationCodeTokenInput {
    client_id: String,
    client_secret: Option<String>,
    code: String,
    redirect_uri: String,
    code_verifier: Option<String>,
}

impl OAuthAuthorizationCodeTokenInput {
    fn from_request(
        request: &OAuthTokenRequest,
        header_credentials: Option<(String, String)>,
    ) -> Option<Self> {
        let client_id = header_credentials
            .as_ref()
            .map(|(client_id, _)| client_id.clone())
            .or_else(|| trimmed_non_empty(request.client_id.as_deref()))?;
        let client_secret = header_credentials
            .map(|(_, client_secret)| client_secret)
            .or_else(|| trimmed_non_empty(request.client_secret.as_deref()));
        let code = trimmed_non_empty(request.code.as_deref())?;
        let redirect_uri = trimmed_non_empty(request.redirect_uri.as_deref())?;
        let code_verifier = trimmed_non_empty(request.code_verifier.as_deref());
        Some(Self {
            client_id,
            client_secret,
            code,
            redirect_uri,
            code_verifier,
        })
    }
}

fn trimmed_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn client_secret_matches_app(app: &OAuthAppRow, client_secret: Option<&str>) -> bool {
    client_secret.is_none_or(|client_secret| app.client_secret == client_secret)
}

fn authorization_code_matches_request(
    code_row: &OAuthAuthorizationCodeRow,
    app: &OAuthAppRow,
    redirect_uri: &str,
) -> bool {
    code_row.oauth_app_id == app.id && code_row.redirect_uri == redirect_uri
}

fn authorization_code_allows_client(
    code_row: &OAuthAuthorizationCodeRow,
    input: &OAuthAuthorizationCodeTokenInput,
) -> bool {
    if let Some(challenge) = code_row.code_challenge.as_deref() {
        return input.code_verifier.as_deref().is_some_and(|verifier| {
            pkce_verifier_matches(
                verifier,
                challenge,
                code_row.code_challenge_method.as_deref(),
            )
        });
    }
    input.client_secret.is_some()
}

async fn oauth_authorization_code_token_response(
    db: &D1Database,
    request: OAuthTokenRequest,
    header_credentials: Option<(String, String)>,
) -> Result<Response> {
    let Some(input) = OAuthAuthorizationCodeTokenInput::from_request(&request, header_credentials)
    else {
        return oauth_invalid_client_response();
    };
    let Some(app) = find_oauth_app_by_client_id(db, &input.client_id).await? else {
        return oauth_invalid_client_response();
    };
    if !client_secret_matches_app(&app, input.client_secret.as_deref()) {
        return oauth_invalid_client_response();
    }
    let Some(code_row) = load_oauth_authorization_code(db, &input.code).await? else {
        return oauth_invalid_client_response();
    };
    if !authorization_code_matches_request(&code_row, &app, &input.redirect_uri) {
        return oauth_invalid_client_response();
    }
    if code_row.expires_at < now_unix_timestamp() {
        delete_oauth_authorization_code(db, &code_row.code).await?;
        return oauth_invalid_client_response();
    }
    if !authorization_code_allows_client(&code_row, &input) {
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
        .any(|value| redirect_uri_matches_registered(value, redirect_uri))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_fallback_body_is_browser_renderable_html() {
        let body = redirect_fallback_body("https://phanpy.social?code=abc&state=a&b");

        assert!(body.starts_with("<!doctype html>"));
        assert!(body.contains("<meta http-equiv=\"refresh\""));
        assert!(body.contains("https://phanpy.social?code=abc&amp;state=a&amp;b"));
    }

    #[test]
    fn create_app_request_from_json_payload_extracts_redirect_variants() {
        let payload = serde_json::json!({
            "client_name": "Client",
            "scopes": "read write",
            "website": "https://example.com",
            "redirect_uris": [
                "https://client.example/callback",
                42,
                "app://callback"
            ],
            "redirect_uri": "https://fallback.example/callback"
        });

        let (request, redirect_uris) =
            create_app_request_from_json_payload(payload).expect("app request");

        assert_eq!(request.client_name.as_deref(), Some("Client"));
        assert_eq!(request.scopes.as_deref(), Some("read write"));
        assert_eq!(request.website.as_deref(), Some("https://example.com"));
        assert_eq!(
            redirect_uris,
            vec!["https://client.example/callback", "app://callback"]
        );
    }

    #[test]
    fn create_app_request_from_json_payload_falls_back_to_redirect_uri() {
        let payload = serde_json::json!({
            "client_name": "Client",
            "redirect_uri": "https://client.example/callback"
        });

        let (_, redirect_uris) =
            create_app_request_from_json_payload(payload).expect("app request");

        assert_eq!(redirect_uris, vec!["https://client.example/callback"]);
    }

    #[test]
    fn parsed_create_app_request_normalizes_fields() {
        let parsed = parsed_create_app_request(
            CreateAppRequest {
                client_name: Some("  Client  ".to_owned()),
                scopes: Some("read write read".to_owned()),
                website: Some(" https://example.com/app ".to_owned()),
            },
            vec![
                " https://client.example/callback app://callback ".to_owned(),
                "app://callback".to_owned(),
            ],
        )
        .expect("parsed app request");

        assert_eq!(parsed.client_name, "Client");
        assert_eq!(parsed.website.as_deref(), Some("https://example.com/app"));
        assert_eq!(parsed.scopes, vec!["read", "write"]);
        assert_eq!(
            parsed.redirect_uris,
            vec!["https://client.example/callback", "app://callback"]
        );
    }

    #[test]
    fn parsed_create_app_request_rejects_blank_client_name() {
        let error = parsed_create_app_request(
            CreateAppRequest {
                client_name: Some("  ".to_owned()),
                ..CreateAppRequest::default()
            },
            vec!["https://client.example/callback".to_owned()],
        )
        .expect_err("blank client name");

        assert_eq!(error, "Validation failed: Name can't be blank");
    }

    #[test]
    fn authorization_code_token_input_prefers_header_credentials() {
        let request = OAuthTokenRequest {
            client_id: Some(" body-client ".to_owned()),
            client_secret: Some(" body-secret ".to_owned()),
            code: Some(" code-1 ".to_owned()),
            redirect_uri: Some(" https://client.example/callback ".to_owned()),
            code_verifier: Some(" verifier ".to_owned()),
            ..OAuthTokenRequest::default()
        };

        let input = OAuthAuthorizationCodeTokenInput::from_request(
            &request,
            Some(("header-client".to_owned(), "header-secret".to_owned())),
        )
        .expect("token input");

        assert_eq!(input.client_id, "header-client");
        assert_eq!(input.client_secret.as_deref(), Some("header-secret"));
        assert_eq!(input.code, "code-1");
        assert_eq!(input.redirect_uri, "https://client.example/callback");
        assert_eq!(input.code_verifier.as_deref(), Some("verifier"));
    }

    #[test]
    fn authorization_code_token_input_requires_core_fields() {
        let request = OAuthTokenRequest {
            client_id: Some("client".to_owned()),
            redirect_uri: Some("https://client.example/callback".to_owned()),
            ..OAuthTokenRequest::default()
        };

        assert!(OAuthAuthorizationCodeTokenInput::from_request(&request, None).is_none());
    }

    #[test]
    fn authorization_code_allows_secret_or_valid_pkce() {
        let code_row = OAuthAuthorizationCodeRow {
            code: "code-1".to_owned(),
            oauth_app_id: 1,
            account_id: "acct-1".to_owned(),
            redirect_uri: "https://client.example/callback".to_owned(),
            scopes_json: "[\"read\"]".to_owned(),
            code_challenge: Some(pkce_code_challenge("verifier", Some("S256"))),
            code_challenge_method: Some("S256".to_owned()),
            expires_at: i64::MAX,
        };
        let pkce_input = OAuthAuthorizationCodeTokenInput {
            client_id: "client".to_owned(),
            client_secret: None,
            code: "code-1".to_owned(),
            redirect_uri: "https://client.example/callback".to_owned(),
            code_verifier: Some("verifier".to_owned()),
        };
        let bad_pkce_input = OAuthAuthorizationCodeTokenInput {
            code_verifier: Some("wrong".to_owned()),
            ..pkce_input.clone()
        };
        let secret_only_code = OAuthAuthorizationCodeRow {
            code_challenge: None,
            code_challenge_method: None,
            ..code_row.clone()
        };
        let secret_input = OAuthAuthorizationCodeTokenInput {
            client_secret: Some("secret".to_owned()),
            code_verifier: None,
            ..pkce_input.clone()
        };

        assert!(authorization_code_allows_client(&code_row, &pkce_input));
        assert!(!authorization_code_allows_client(
            &code_row,
            &bad_pkce_input
        ));
        assert!(authorization_code_allows_client(
            &secret_only_code,
            &secret_input
        ));
        assert!(!authorization_code_allows_client(
            &secret_only_code,
            &pkce_input
        ));
    }

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
    fn app_bearer_token_lookup_sql_only_matches_client_secret() {
        assert!(FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL.contains("WHERE client_secret = ?1"));
        assert!(!FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL.contains("client_id = ?1"));
    }

    #[test]
    fn client_secret_and_code_binding_match_expected_app() {
        let app = oauth_app_fixture();
        let code_row = OAuthAuthorizationCodeRow {
            code: "code-1".to_owned(),
            oauth_app_id: 7,
            account_id: "acct-1".to_owned(),
            redirect_uri: "https://client.example/callback".to_owned(),
            scopes_json: "[\"read\"]".to_owned(),
            code_challenge: None,
            code_challenge_method: None,
            expires_at: i64::MAX,
        };

        assert!(client_secret_matches_app(&app, Some("secret")));
        assert!(client_secret_matches_app(&app, None));
        assert!(!client_secret_matches_app(&app, Some("wrong")));
        assert!(authorization_code_matches_request(
            &code_row,
            &app,
            "https://client.example/callback"
        ));
        assert!(!authorization_code_matches_request(
            &code_row,
            &app,
            "https://other.example/callback"
        ));
    }

    #[test]
    fn access_authorize_actions_match_expected_flow() {
        assert_eq!(
            access_authorize_get_action(false, false),
            AccessAuthorizeGetAction::RedirectToLogin
        );
        assert_eq!(
            access_authorize_get_action(false, true),
            AccessAuthorizeGetAction::MissingLinkedAccount
        );
        assert_eq!(
            access_authorize_get_action(true, false),
            AccessAuthorizeGetAction::ShowConsent
        );

        assert_eq!(
            access_authorize_post_action(false, false, false),
            AccessAuthorizePostAction::RedirectToLogin
        );
        assert_eq!(
            access_authorize_post_action(true, false, false),
            AccessAuthorizePostAction::ShowConsent
        );
        assert_eq!(
            access_authorize_post_action(true, true, false),
            AccessAuthorizePostAction::IssueCode
        );
        assert_eq!(
            access_authorize_post_action(true, false, true),
            AccessAuthorizePostAction::IssueCode
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
        );

        assert!(body.contains("name=\"username\""));
        assert!(body.contains("type=\"password\""));
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
        );

        assert!(!body.contains("name=\"username\""));
        assert!(!body.contains("type=\"password\""));
        assert!(body.contains("Authorize Client"));
        assert!(body.contains("name=\"client_id\" value=\"client\""));
        assert!(body.contains("name=\"scope\" value=\"read write\""));
        assert!(body.contains("name=\"code_challenge_method\" value=\"S256\""));
        assert!(body.contains("name=\"approve\" value=\"true\""));
        assert!(body.contains("Approve access for this application"));
    }
}
