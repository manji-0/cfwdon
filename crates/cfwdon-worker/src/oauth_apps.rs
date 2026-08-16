use crate::auth::{AUTH0_REFRESH_COOKIE, AUTH0_SESSION_COOKIE};
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
use worker::{FormData, ResponseBody, d1::D1Type};
use worker::{Request, Response, Result, RouteContext};

use crate::D1Database;
mod auth0_callback;
mod authorize_route;
mod authorize_validation;
mod token_routes;

pub(crate) use auth0_callback::{auth0_callback_response, exchange_auth0_refresh_token};
pub(crate) use authorize_route::oauth_authorize_response;
pub(in crate::oauth_apps) use authorize_route::{
    access_authenticated_without_account_response, oauth_authorize_error_response,
};
pub(in crate::oauth_apps) use token_routes::requested_oauth_token_scopes;
pub(crate) use token_routes::{oauth_revoke_response, oauth_token_response};

use authorize_validation::code_challenge_method_is_supported;
const AUTHORIZATION_CODE_TTL_SECONDS: i64 = 600;
const APP_ACCESS_TOKEN_TTL_SECONDS: i64 = 3600;
const AUTH0_ACCESS_TOKEN_COOKIE_TTL_SECONDS: i64 = 3600;
pub(crate) const AUTH0_WEB_SESSION_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
pub(in crate::oauth_apps) const OAUTH_AUTHORIZE_CSRF_COOKIE: &str = "cfwdon_oauth_authorize_csrf";
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

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Auth0TokenResponse {
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    #[serde(default)]
    pub(crate) expires_in: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Auth0AuthorizeStateCookie {
    state: String,
    code_verifier: String,
    return_url: String,
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

pub(in crate::oauth_apps) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
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

pub(in crate::oauth_apps) async fn load_account_password_hash(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<String>> {
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
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let token = token.trim();
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
        .await
        .and_then(|__d1| crate::d1_results::<OAuthAppRow>(&__d1))
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
    let now_binding = D1Type::Integer(i32::try_from(now_unix_timestamp()).unwrap_or(i32::MAX));
    let legacy_only = column == "t.access_token";
    let legacy_guard = if legacy_only {
        " AND t.access_token_hash IS NULL"
    } else {
        ""
    };
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
               AND (t.expires_at IS NULL OR t.expires_at > ?2){legacy_guard}
             LIMIT 1"
    );
    let Some(row) = db
        .prepare(&sql)
        .bind_refs(&[binding, now_binding])?
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
                .map(LocalAccount::from_record)
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
    db.prepare(LEGACY_OAUTH_ACCESS_TOKEN_MIGRATE_SQL)
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    Ok(())
}

const LEGACY_OAUTH_ACCESS_TOKEN_MIGRATE_SQL: &str = "UPDATE oauth_access_tokens
         SET access_token_hash = ?1,
             access_token = ?2
         WHERE access_token = ?3
           AND access_token_hash IS NULL";

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

pub(in crate::oauth_apps) async fn issue_oauth_authorization_code(
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

pub(in crate::oauth_apps) fn authorization_redirect_with_params(
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
        .append_pair("scope", "openid profile email offline_access")
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

pub(in crate::oauth_apps) fn redirect_response(location: &str) -> Result<Response> {
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

pub(in crate::oauth_apps) fn html_response(body: &str, status: u16) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(body.as_bytes().to_vec()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/html; charset=utf-8")?;
    Ok(response.with_status(status))
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

pub(in crate::oauth_apps) fn request_cookie_value(
    req: &Request,
    name: &str,
) -> Result<Option<String>> {
    let Some(cookie_header) = req.headers().get("Cookie")? else {
        return Ok(None);
    };
    Ok(cookie_header.split(';').find_map(|part| {
        let (cookie_name, value) = part.trim().split_once('=')?;
        (cookie_name == name && !value.trim().is_empty()).then(|| value.trim().to_owned())
    }))
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

pub(crate) fn access_token_cookie_max_age(expires_in: Option<i64>) -> i64 {
    expires_in
        .filter(|value| *value > 0)
        .unwrap_or(AUTH0_ACCESS_TOKEN_COOKIE_TTL_SECONDS)
}

fn auth0_session_cookie(name: &str, value: &str, max_age: i64) -> String {
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age={max_age}")
}

pub(crate) fn set_auth0_session_cookies(
    response: &mut Response,
    access_token: &str,
    refresh_token: Option<&str>,
    access_max_age: i64,
) -> Result<()> {
    response.headers_mut().append(
        "Set-Cookie",
        &auth0_session_cookie(AUTH0_SESSION_COOKIE, access_token, access_max_age),
    )?;
    if let Some(refresh_token) = refresh_token.filter(|value| !value.is_empty()) {
        response.headers_mut().append(
            "Set-Cookie",
            &auth0_session_cookie(
                AUTH0_REFRESH_COOKIE,
                refresh_token,
                AUTH0_WEB_SESSION_TTL_SECONDS,
            ),
        )?;
    }
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(())
}

fn clear_auth0_session_cookie(response: &mut Response) -> Result<()> {
    response.headers_mut().append(
        "Set-Cookie",
        &auth0_session_cookie(AUTH0_SESSION_COOKIE, "", 0),
    )?;
    response.headers_mut().append(
        "Set-Cookie",
        &auth0_session_cookie(AUTH0_REFRESH_COOKIE, "", 0),
    )?;
    response.headers_mut().set("Cache-Control", "no-store")?;
    Ok(())
}

pub(crate) fn auth0_logout_redirect_response(config: &cfwdon_core::AppConfig) -> Result<Response> {
    let logout_url = auth0_logout_url(config).map_err(worker::Error::RustError)?;
    let mut response = redirect_response(logout_url.as_str())?;
    clear_auth0_session_cookie(&mut response)?;
    Ok(response)
}

pub(crate) fn auth0_relogin_redirect_response(
    config: &cfwdon_core::AppConfig,
    return_url: &Url,
) -> Result<Response> {
    let mut response = auth0_login_redirect_response(config, return_url, return_url)?;
    clear_auth0_session_cookie(&mut response)?;
    Ok(response)
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum OAuthAuthorizeFailure {
    Html {
        message: String,
    },
    Redirect {
        redirect_uri: String,
        state: Option<String>,
        error: &'static str,
        description: String,
    },
}

#[cfg_attr(not(test), allow(dead_code))]
fn oauth_access_token_is_unexpired(expires_at: Option<i64>, now: i64) -> bool {
    expires_at.is_none_or(|expires_at| expires_at > now)
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
    db: &crate::D1Database,
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
    if !code_challenge_method_is_supported(method) {
        return false;
    }
    constant_time_eq(
        pkce_code_challenge(verifier, method).as_bytes(),
        challenge.as_bytes(),
    )
}

pub(crate) async fn create_app_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let request = match parse_create_app_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(&message, 422),
    };
    let app = insert_oauth_app(&db, &request).await?;
    Response::from_json(&app_document(&app, &config))
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
    fn access_token_cookie_ttl_prefers_auth0_expires_in() {
        assert_eq!(access_token_cookie_max_age(Some(7200)), 7200);
        assert_eq!(access_token_cookie_max_age(Some(0)), 3600);
        assert_eq!(access_token_cookie_max_age(None), 3600);
    }

    #[test]
    fn web_session_refresh_cookie_lasts_seven_days() {
        let cookie = auth0_session_cookie(
            AUTH0_REFRESH_COOKIE,
            "refresh-1",
            AUTH0_WEB_SESSION_TTL_SECONDS,
        );
        assert!(cookie.contains("Max-Age=604800"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("cfwdon_auth0_refresh_token=refresh-1"));
    }

    #[test]
    fn pkce_verifier_rejects_stored_plain_method() {
        let challenge = "verifier-value";
        assert!(!pkce_verifier_matches(
            "verifier-value",
            challenge,
            Some("plain")
        ));
        assert!(pkce_verifier_matches(
            "verifier",
            &pkce_code_challenge("verifier", Some("S256")),
            Some("S256")
        ));
    }

    #[test]
    fn bearer_authorization_header_is_case_insensitive() {
        assert_eq!(
            parse_bearer_authorization_header("Bearer tok-1").as_deref(),
            Some("tok-1")
        );
        assert_eq!(
            parse_bearer_authorization_header("bearer tok-1").as_deref(),
            Some("tok-1")
        );
        assert_eq!(
            parse_bearer_authorization_header("BEARER tok-1").as_deref(),
            Some("tok-1")
        );
        assert_eq!(
            parse_bearer_authorization_header("Bearer  tok-1").as_deref(),
            Some("tok-1")
        );
        assert_eq!(parse_bearer_authorization_header("Basic abc"), None);
    }

    #[test]
    fn legacy_oauth_access_token_migrate_clears_plaintext_column() {
        assert!(LEGACY_OAUTH_ACCESS_TOKEN_MIGRATE_SQL.contains("access_token_hash = ?1"));
        assert!(LEGACY_OAUTH_ACCESS_TOKEN_MIGRATE_SQL.contains("access_token = ?2"));
        assert!(LEGACY_OAUTH_ACCESS_TOKEN_MIGRATE_SQL.contains("access_token_hash IS NULL"));
    }

    #[test]
    fn oauth_access_token_expiry_null_means_no_expiry() {
        assert!(oauth_access_token_is_unexpired(None, 1_700_000_000));
        assert!(oauth_access_token_is_unexpired(
            Some(1_700_000_001),
            1_700_000_000
        ));
        assert!(!oauth_access_token_is_unexpired(
            Some(1_699_999_999),
            1_700_000_000
        ));
    }
}
