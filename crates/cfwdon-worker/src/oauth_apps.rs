use crate::auth::{AUTH0_SESSION_COOKIE, find_account_by_email};
use crate::auth::{find_account_by_id, find_account_by_username};
use crate::id_utils::generate_entity_id;
use crate::runtime_config::load_config;
use crate::time_html::{escape_html, now_unix_timestamp};
use crate::verify_auth0_jwt;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use cfwdon_domain::LocalAccount;
use pbkdf2::pbkdf2_hmac_array;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use worker::{D1Database, Fetch, FormData, Headers, Method, RequestInit, ResponseBody, d1::D1Type};
use worker::{Request, Response, Result, RouteContext};

const AUTHORIZATION_CODE_TTL_SECONDS: i64 = 600;
const APP_ACCESS_TOKEN_TTL_SECONDS: i64 = 3600;
const OAUTH_AUTHORIZE_CSRF_COOKIE: &str = "cfwdon_oauth_authorize_csrf";
const AUTH0_AUTHORIZE_STATE_COOKIE: &str = "cfwdon_auth0_authorize";
const PASSWORD_HASH_ALGORITHM: &str = "pbkdf2-sha256";
const PASSWORD_HASH_ITERATIONS: u32 = 210_000;
const FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL: &str =
    "SELECT a.id, a.name, a.website, a.scopes_json, a.redirect_uri_legacy, a.redirect_uris_json,
                a.client_id, a.client_secret, a.client_secret_expires_at
         FROM oauth_app_access_tokens t
         INNER JOIN oauth_apps a ON a.id = t.oauth_app_id
         WHERE t.access_token_hash = ?1
           AND t.expires_at > ?2
         ORDER BY a.id ASC
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
    csrf_token: Option<String>,
    authorize: OAuthAuthorizeRequest,
}

#[derive(Debug, Deserialize)]
struct Auth0CallbackRequest {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Auth0TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Auth0AuthorizeStateCookie {
    state: String,
    code_verifier: String,
    return_url: String,
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

fn build_oauth_token_document_with_expires_in(
    access_token: &str,
    scope: &str,
    expires_in: i64,
) -> serde_json::Value {
    let mut document = build_oauth_token_document(access_token, scope);
    document["expires_in"] = serde_json::json!(expires_in);
    document
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

pub(crate) fn oauth_bearer_token_hash(token: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(token.as_bytes()))
}

pub(crate) async fn find_oauth_app_by_bearer_token(
    db: &D1Database,
    token: &str,
) -> Result<Option<OAuthAppRow>> {
    let token_hash = oauth_bearer_token_hash(token);
    let binding = D1Type::Text(token_hash.as_str());
    let expires_at_binding =
        D1Type::Integer(i32::try_from(now_unix_timestamp()).unwrap_or(i32::MAX));
    if let Some(app) = db
        .prepare(FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL)
        .bind_refs(&[binding, expires_at_binding])?
        .first::<OAuthAppRow>(None)
        .await?
    {
        return Ok(Some(app));
    }

    let legacy_binding = D1Type::Text(token);
    let expires_at_binding =
        D1Type::Integer(i32::try_from(now_unix_timestamp()).unwrap_or(i32::MAX));
    let Some(app) = db
        .prepare(
            "SELECT a.id, a.name, a.website, a.scopes_json, a.redirect_uri_legacy, a.redirect_uris_json,
                    a.client_id, a.client_secret, a.client_secret_expires_at
             FROM oauth_app_access_tokens t
             INNER JOIN oauth_apps a ON a.id = t.oauth_app_id
             WHERE t.access_token = ?1
               AND t.expires_at > ?2
             ORDER BY a.id ASC
             LIMIT 1",
        )
        .bind_refs(&[legacy_binding, expires_at_binding])?
        .first::<OAuthAppRow>(None)
        .await?
    else {
        return Ok(None);
    };
    migrate_legacy_oauth_app_access_token_hash(db, token, &token_hash).await?;
    Ok(Some(app))
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
    let token_hash = oauth_bearer_token_hash(token);
    if let Some(auth) = find_oauth_access_token_with_account_by_token_hash(db, &token_hash).await? {
        return Ok(Some(auth));
    }
    let Some(auth) = find_legacy_oauth_access_token_with_account_by_plaintext(db, token).await?
    else {
        return Ok(None);
    };
    migrate_legacy_oauth_access_token_hash(db, token, &token_hash).await?;
    Ok(Some(auth))
}

async fn find_oauth_access_token_with_account_by_token_hash(
    db: &D1Database,
    token_hash: &str,
) -> Result<Option<OAuthAccessTokenWithAccount>> {
    find_oauth_access_token_with_account_by_column(db, "t.access_token_hash", token_hash).await
}

async fn find_legacy_oauth_access_token_with_account_by_plaintext(
    db: &D1Database,
    token: &str,
) -> Result<Option<OAuthAccessTokenWithAccount>> {
    find_oauth_access_token_with_account_by_column(db, "t.access_token", token).await
}

async fn find_oauth_access_token_with_account_by_column(
    db: &D1Database,
    column: &str,
    value: &str,
) -> Result<Option<OAuthAccessTokenWithAccount>> {
    let binding = D1Type::Text(value);
    let sql = format!(
        "SELECT t.access_token_hash AS access_token,
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
                    '' AS private_key_jwk,
                    a.public_key_pem,
                    a.created_at
             FROM oauth_access_tokens t
             LEFT JOIN accounts a ON a.id = t.account_id
             WHERE {column} = ?1
             LIMIT 1"
    );
    let Some(row) = db
        .prepare(&sql)
        .bind_refs(&[binding])?
        .first::<serde_json::Value>(None)
        .await?
    else {
        return Ok(None);
    };

    oauth_access_token_auth_from_joined_row(row)
}

fn oauth_access_token_auth_from_joined_row(
    row: serde_json::Value,
) -> Result<Option<OAuthAccessTokenWithAccount>> {
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

async fn migrate_legacy_oauth_access_token_hash(
    db: &D1Database,
    token: &str,
    token_hash: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(token_hash),
        D1Type::Text(token_hash),
        D1Type::Text(token),
    ];
    db.prepare(
        "UPDATE oauth_access_tokens
         SET access_token = ?1,
             access_token_hash = ?2
         WHERE access_token = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn migrate_legacy_oauth_app_access_token_hash(
    db: &D1Database,
    token: &str,
    token_hash: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(token_hash),
        D1Type::Text(token_hash),
        D1Type::Text(token),
    ];
    db.prepare(
        "UPDATE oauth_app_access_tokens
         SET access_token = ?1,
             access_token_hash = ?2
         WHERE access_token = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn issue_oauth_access_token(
    db: &D1Database,
    oauth_app_id: i64,
    account_id: &str,
    scopes: &[String],
) -> Result<OAuthAccessTokenRow> {
    let access_token = generate_entity_id(32)?;
    let access_token_hash = oauth_bearer_token_hash(&access_token);
    let scopes_json = serde_json::to_string(scopes).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize access token scopes: {error}"))
    })?;
    let bindings = [
        D1Type::Text(access_token_hash.as_str()),
        D1Type::Text(access_token_hash.as_str()),
        D1Type::Integer(i32::try_from(oauth_app_id).unwrap_or(i32::MAX)),
        D1Type::Text(account_id),
        D1Type::Text(scopes_json.as_str()),
    ];
    db.prepare(
        "INSERT INTO oauth_access_tokens (
            access_token,
            access_token_hash,
            oauth_app_id,
            account_id,
            scopes_json,
            created_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
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

async fn issue_oauth_app_access_token(
    db: &D1Database,
    oauth_app_id: i64,
    scopes: &[String],
) -> Result<String> {
    let access_token = generate_entity_id(32)?;
    let access_token_hash = oauth_bearer_token_hash(&access_token);
    let scopes_json = serde_json::to_string(scopes).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize app token scopes: {error}"))
    })?;
    let expires_at = now_unix_timestamp() + APP_ACCESS_TOKEN_TTL_SECONDS;
    let bindings = [
        D1Type::Text(access_token_hash.as_str()),
        D1Type::Text(access_token_hash.as_str()),
        D1Type::Integer(i32::try_from(oauth_app_id).unwrap_or(i32::MAX)),
        D1Type::Text(scopes_json.as_str()),
        D1Type::Integer(i32::try_from(expires_at).unwrap_or(i32::MAX)),
    ];
    db.prepare(
        "INSERT INTO oauth_app_access_tokens (
            access_token,
            access_token_hash,
            oauth_app_id,
            scopes_json,
            expires_at,
            created_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(access_token)
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

pub(crate) fn auth0_login_configured(config: &cfwdon_core::AppConfig) -> bool {
    !config.auth0_domain.trim().is_empty()
        && !config.auth0_client_id.trim().is_empty()
        && !config.auth0_audience.trim().is_empty()
}

pub(crate) fn auth0_login_url(
    config: &cfwdon_core::AppConfig,
    callback_url: &Url,
    state: &str,
    code_challenge: &str,
) -> std::result::Result<Url, String> {
    let mut login_url = auth0_domain_url(config)?;
    login_url.set_path("/authorize");
    login_url.set_query(None);
    login_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", config.auth0_client_id.trim())
        .append_pair("redirect_uri", callback_url.as_str())
        .append_pair("audience", config.auth0_audience.trim())
        .append_pair("scope", "openid profile email")
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(login_url)
}

pub(crate) fn auth0_logout_url(
    config: &cfwdon_core::AppConfig,
) -> std::result::Result<Url, String> {
    let mut logout_url = auth0_domain_url(config)?;
    logout_url.set_path("/v2/logout");
    logout_url.set_query(None);
    logout_url
        .query_pairs_mut()
        .append_pair("client_id", config.auth0_client_id.trim())
        .append_pair("returnTo", crate::instance_base_url(config).as_str());
    Ok(logout_url)
}

fn auth0_domain_url(config: &cfwdon_core::AppConfig) -> std::result::Result<Url, String> {
    let mut domain = config.auth0_domain.trim().trim_end_matches('/').to_owned();
    if !domain.starts_with("http://") && !domain.starts_with("https://") {
        domain = format!("https://{domain}");
    }
    Url::parse(&domain).map_err(|error| format!("invalid Auth0 domain: {error}"))
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
    let return_url =
        oauth_authorize_url_from_form(base_url, authorize).map_err(worker::Error::RustError)?;
    auth0_login_redirect_response(config, base_url, &return_url)
}

pub(crate) fn auth0_login_redirect_response(
    config: &cfwdon_core::AppConfig,
    base_url: &Url,
    return_url: &Url,
) -> Result<Response> {
    let mut callback_url = base_url.clone();
    callback_url.set_path("/oauth/auth0/callback");
    callback_url.set_query(None);

    let state = generate_entity_id(32)?;
    let code_verifier = generate_entity_id(48)?;
    let code_challenge = pkce_code_challenge(&code_verifier, Some("S256"));
    let login_url = auth0_login_url(config, &callback_url, &state, &code_challenge)
        .map_err(worker::Error::RustError)?;
    let session = Auth0AuthorizeStateCookie {
        state,
        code_verifier,
        return_url: return_url.to_string(),
    };
    let mut response = redirect_response(login_url.as_str())?;
    set_auth0_authorize_state_cookie(&mut response, &session)?;
    Ok(response)
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

fn html_response(body: &str, status: u16) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(body.as_bytes().to_vec()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response.with_status(status))
}

fn oauth_authorize_csrf_cookie(req: &Request) -> Result<Option<String>> {
    request_cookie_value(req, OAUTH_AUTHORIZE_CSRF_COOKIE)
}

fn auth0_authorize_state_cookie(req: &Request) -> Result<Option<Auth0AuthorizeStateCookie>> {
    let Some(value) = request_cookie_value(req, AUTH0_AUTHORIZE_STATE_COOKIE)? else {
        return Ok(None);
    };
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|error| {
        worker::Error::RustError(format!("invalid Auth0 state cookie: {error}"))
    })?;
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        worker::Error::RustError(format!("invalid Auth0 state cookie payload: {error}"))
    })
}

fn request_cookie_value(req: &Request, name: &str) -> Result<Option<String>> {
    let Some(cookie_header) = req.headers().get("Cookie")? else {
        return Ok(None);
    };
    Ok(cookie_header.split(';').find_map(|part| {
        let (cookie_name, value) = part.trim().split_once('=')?;
        (cookie_name == name && !value.trim().is_empty()).then(|| value.trim().to_owned())
    }))
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

fn set_auth0_authorize_state_cookie(
    response: &mut Response,
    session: &Auth0AuthorizeStateCookie,
) -> Result<()> {
    let payload = serde_json::to_vec(session).map_err(|error| {
        worker::Error::RustError(format!("failed to encode Auth0 state cookie: {error}"))
    })?;
    response.headers_mut().append(
        "Set-Cookie",
        &format!(
            "{AUTH0_AUTHORIZE_STATE_COOKIE}={}; Path=/oauth/auth0/callback; HttpOnly; SameSite=Lax; Secure; Max-Age=600",
            URL_SAFE_NO_PAD.encode(payload)
        ),
    )?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(())
}

fn set_auth0_session_cookie(response: &mut Response, access_token: &str) -> Result<()> {
    response.headers_mut().append(
        "Set-Cookie",
        &format!(
            "{AUTH0_SESSION_COOKIE}={access_token}; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age=3600"
        ),
    )?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(())
}

fn clear_auth0_authorize_state_cookie(response: &mut Response) -> Result<()> {
    response.headers_mut().append(
        "Set-Cookie",
        &format!(
            "{AUTH0_AUTHORIZE_STATE_COOKIE}=; Path=/oauth/auth0/callback; HttpOnly; SameSite=Lax; Secure; Max-Age=0"
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
        .map(|message| format!("<p role=\"alert\">{}</p>", escape_html(message)))
        .unwrap_or_default();
    let scope_html = request
        .scope
        .as_deref()
        .map(escape_html)
        .unwrap_or_else(|| "read".to_owned());
    let redirect_uri_html = request
        .redirect_uri
        .as_deref()
        .map(escape_html)
        .unwrap_or_default();
    let app_details = format!(
        "<dl><dt>Application</dt><dd>{}</dd><dt>Scopes</dt><dd>{scope_html}</dd><dt>Redirect URI</dt><dd>{redirect_uri_html}</dd></dl>",
        escape_html(&app.name)
    );
    let form_body = if require_login_credentials {
        format!(
            "{app_details}{hidden}{csrf_input}<label>Username or email <input name=\"username\" autocomplete=\"username\" required></label><label>Password <input name=\"password\" type=\"password\" autocomplete=\"current-password\" required></label><button type=\"submit\">Authorize</button>"
        )
    } else {
        format!(
            "<p>Approve access for this application.</p>{app_details}{hidden}{csrf_input}<input type=\"hidden\" name=\"approve\" value=\"true\"><button type=\"submit\">Authorize</button>"
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
    oauth_authorize_consent_response(request, app, error, true, error.map(|_| 401).unwrap_or(200))
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
    let client_id = request
        .client_id
        .as_deref()
        .ok_or_else(|| "client_id is required".to_owned())?;
    let Some(app) = find_oauth_app_by_client_id(db, client_id)
        .await
        .map_err(|error| format!("failed to load OAuth app: {error}"))?
    else {
        return Err("Unknown OAuth client".to_owned());
    };
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .ok_or_else(|| "redirect_uri is required".to_owned())?;
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

pub(crate) async fn auth0_callback_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
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
    set_auth0_session_cookie(&mut response, &token.access_token)?;
    clear_auth0_authorize_state_cookie(&mut response)?;
    Ok(response)
}

async fn exchange_auth0_authorization_code(
    config: &cfwdon_core::AppConfig,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<Auth0TokenResponse> {
    let mut token_url = auth0_domain_url(config).map_err(worker::Error::RustError)?;
    token_url.set_path("/oauth/token");
    token_url.set_query(None);
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("client_id", config.auth0_client_id.trim())
        .append_pair("code", code)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("code_verifier", code_verifier)
        .finish();
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
            "Auth0 token endpoint rejected authorization code with HTTP {}",
            response.status_code()
        )));
    }
    response.json::<Auth0TokenResponse>().await
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
                    redirect_with_authorization_code(&db, &authorize, &app, &account.id, &scopes)
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
        return redirect_with_authorization_code(&db, &authorize, &app, &account.id, &scopes).await;
    }

    let authorize = match req.query::<OAuthAuthorizeRequest>() {
        Ok(query) => query,
        Err(_) => return oauth_authorize_error_response("Invalid authorization request", 400),
    };
    let (authorize, app, _scopes) = match validate_authorize_request(&db, authorize).await {
        Ok(value) => value,
        Err(message) => return oauth_authorize_error_response(&message, 400),
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

    let access_token = issue_oauth_app_access_token(&db, app.id, &requested_scopes).await?;
    Response::from_json(&build_oauth_token_document_with_expires_in(
        &access_token,
        &requested_scopes.join(" "),
        APP_ACCESS_TOKEN_TTL_SECONDS,
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
    fn app_bearer_token_lookup_sql_uses_app_access_tokens_only() {
        assert!(FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL.contains("oauth_app_access_tokens"));
        assert!(FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL.contains("t.access_token_hash = ?1"));
        assert!(FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL.contains("t.expires_at > ?2"));
        assert!(!FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL.contains("client_secret = ?1"));
        assert!(!FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL.contains("client_id = ?1"));
    }

    #[test]
    fn oauth_bearer_token_hash_is_stable_and_non_plaintext() {
        let hash = oauth_bearer_token_hash("plain-token");

        assert_eq!(
            hash,
            "sha256:23fb79e20d37abf2418d78115eb0cc8c74b52f4ed8b91dda7fc03a1d41fc15e3"
        );
        assert!(!hash.contains("plain-token"));
    }

    #[test]
    fn app_token_document_can_include_expiry_without_exposing_client_secret() {
        let document = build_oauth_token_document_with_expires_in("issued-token", "read", 3600);

        assert_eq!(document["access_token"], "issued-token");
        assert_eq!(document["scope"], "read");
        assert_eq!(document["expires_in"], 3600);
        assert_ne!(document["access_token"], "secret");
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
        assert!(body.contains("<dt>Redirect URI</dt><dd>https://client.example/callback</dd>"));
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
        assert!(body.contains("<dt>Scopes</dt><dd>read write</dd>"));
        assert!(body.contains("Approve access for this application"));
    }
}
