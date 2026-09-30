use crate::time_html::now_unix_timestamp;
use crate::tracked_d1::D1Database;
use cfwdon_domain::{LocalAccount, LocalAccountRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use worker::{Fetch, Headers, Method, Request, RequestInit, Response, Result, d1::D1Type};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OAuthAccessTokenRow {
    pub(crate) access_token: String,
    pub(crate) oauth_app_id: i64,
    pub(crate) scopes_json: String,
}

pub(crate) fn oauth_access_token_has_any_scope(row: &OAuthAccessTokenRow, scopes: &[&str]) -> bool {
    oauth_access_token_has_any_scope_json(&row.scopes_json, scopes)
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

pub(crate) fn access_token_cookie_max_age(expires_in: Option<i64>) -> i64 {
    expires_in
        .filter(|value| *value > 0)
        .unwrap_or(AUTH0_ACCESS_TOKEN_COOKIE_TTL_SECONDS)
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

pub(crate) async fn post_auth0_token_form(
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

pub(crate) const AUTH0_SESSION_COOKIE: &str = "cfwdon_auth0_access_token";

pub(crate) const AUTH0_REFRESH_COOKIE: &str = "cfwdon_auth0_refresh_token";

pub(crate) async fn link_oauth_app_to_account(
    db: &D1Database,
    oauth_app_id: i64,
    account_id: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Integer(oauth_app_id as i32),
        D1Type::Text(account_id),
    ];
    db.prepare(
        "INSERT OR REPLACE INTO oauth_app_accounts (
            oauth_app_id,
            account_id,
            created_at
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

pub(crate) const AUTH0_ACCESS_TOKEN_COOKIE_TTL_SECONDS: i64 = 3600;

pub(crate) const AUTH0_WEB_SESSION_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;

pub(crate) const FIND_OAUTH_APP_BY_BEARER_TOKEN_SQL: &str =
    "SELECT a.id, a.name, a.website, a.scopes_json, a.redirect_uri_legacy, a.redirect_uris_json,
                a.client_id, a.client_secret, a.client_secret_expires_at
         FROM oauth_app_access_tokens t
         INNER JOIN oauth_apps a ON a.id = t.oauth_app_id
         WHERE t.access_token_hash = ?1
           AND t.expires_at > ?2
         ORDER BY a.id ASC
         LIMIT 1";

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Auth0TokenResponse {
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    #[serde(default)]
    pub(crate) expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthAppRow {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) website: Option<String>,
    pub(crate) scopes_json: String,
    pub(crate) redirect_uri_legacy: String,
    pub(crate) redirect_uris_json: String,
    pub(crate) client_id: String,
    pub(crate) client_secret: String,
    pub(crate) client_secret_expires_at: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct OAuthAccessTokenWithAccount {
    pub(crate) token: OAuthAccessTokenRow,
    pub(crate) account: Option<LocalAccount>,
}

pub(crate) fn oauth_access_token_has_any_scope_json(scopes_json: &str, scopes: &[&str]) -> bool {
    serde_json::from_str::<Vec<String>>(scopes_json)
        .unwrap_or_default()
        .iter()
        .any(|scope| scopes.contains(&scope.as_str()))
}

pub(crate) fn oauth_bearer_token_hash(token: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(token.as_bytes()))
}

pub(crate) async fn find_oauth_access_token_with_account_by_token_hash(
    db: &D1Database,
    token_hash: &str,
) -> Result<Option<OAuthAccessTokenWithAccount>> {
    find_oauth_access_token_with_account_by_column(db, "t.access_token_hash", token_hash).await
}

pub(crate) async fn find_legacy_oauth_access_token_with_account_by_plaintext(
    db: &D1Database,
    token: &str,
) -> Result<Option<OAuthAccessTokenWithAccount>> {
    find_oauth_access_token_with_account_by_column(db, "t.access_token", token).await
}

pub(crate) async fn migrate_legacy_oauth_access_token_hash(
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

pub(crate) async fn migrate_legacy_oauth_app_access_token_hash(
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

pub(crate) fn auth0_domain_url(
    config: &cfwdon_core::AppConfig,
) -> std::result::Result<Url, String> {
    let mut domain = config.auth0_domain.trim().trim_end_matches('/').to_owned();
    if !domain.starts_with("http://") && !domain.starts_with("https://") {
        domain = format!("https://{domain}");
    }
    Url::parse(&domain).map_err(|error| format!("invalid Auth0 domain: {error}"))
}

pub(crate) fn auth0_session_cookie(name: &str, value: &str, max_age: i64) -> String {
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age={max_age}")
}

pub(crate) async fn find_oauth_access_token_with_account_by_column(
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

pub(crate) const LEGACY_OAUTH_ACCESS_TOKEN_MIGRATE_SQL: &str = "UPDATE oauth_access_tokens
         SET access_token_hash = ?1,
             access_token = ?2
         WHERE access_token = ?3
           AND access_token_hash IS NULL";

pub(crate) fn oauth_access_token_auth_from_joined_row(
    row: serde_json::Value,
) -> Result<Option<OAuthAccessTokenWithAccount>> {
    let token = serde_json::from_value::<OAuthAccessTokenRow>(row.clone()).map_err(|error| {
        worker::Error::RustError(format!("failed to decode OAuth access token row: {error}"))
    })?;
    let account = match row.get("id").and_then(serde_json::Value::as_str) {
        Some(_) => Some(
            serde_json::from_value::<LocalAccountRecord>(row)
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

#[cfg(test)]
mod unit_tests;
