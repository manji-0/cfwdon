use super::{
    APP_ACCESS_TOKEN_TTL_SECONDS, OAuthAppRow, OAuthAuthorizationCodeRow,
    build_oauth_token_document, delete_oauth_authorization_code, find_oauth_app_by_client_id,
    issue_oauth_access_token, issue_oauth_app_access_token, load_oauth_authorization_code,
    oauth_app_redirect_uris, oauth_app_scopes, oauth_bearer_token_hash,
    parse_basic_authorization_header, pkce_verifier_matches, redirect_uri_matches_registered,
};
use crate::D1Database;
use crate::auth::find_account_by_id;
use crate::runtime_config::load_config;
use crate::time_html::now_unix_timestamp;
use serde::Deserialize;
use worker::{Request, Response, Result, RouteContext, d1::D1Type};

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

fn build_oauth_token_document_with_expires_in(
    access_token: &str,
    scope: &str,
    expires_in: i64,
) -> serde_json::Value {
    let mut document = build_oauth_token_document(access_token, scope);
    document["expires_in"] = serde_json::json!(expires_in);
    document
}

fn oauth_invalid_client_response() -> Result<Response> {
    with_oauth_token_cache_headers(
        Response::from_json(&serde_json::json!({
            "error": "invalid_client",
            "error_description": "Client authentication failed due to unknown client, no client authentication included, or unsupported authentication method.",
        }))?
        .with_status(oauth_invalid_client_status()),
    )
}

fn oauth_invalid_client_status() -> u16 {
    401
}

fn oauth_invalid_grant_response(description: &str) -> Result<Response> {
    with_oauth_token_cache_headers(
        Response::from_json(&serde_json::json!({
            "error": "invalid_grant",
            "error_description": description,
        }))?
        .with_status(oauth_invalid_grant_status()),
    )
}

fn oauth_invalid_grant_status() -> u16 {
    400
}

fn oauth_invalid_scope_response() -> Result<Response> {
    with_oauth_token_cache_headers(
        Response::from_json(&serde_json::json!({
            "error": "invalid_scope",
            "error_description": "The requested scope is invalid, unknown, or malformed.",
        }))?
        .with_status(400),
    )
}

fn oauth_unsupported_grant_type_response() -> Result<Response> {
    with_oauth_token_cache_headers(
        Response::from_json(&serde_json::json!({
            "error": "unsupported_grant_type",
            "error_description": "The authorization grant type is not supported by the authorization server.",
        }))?
        .with_status(400),
    )
}

fn oauth_invalid_request_response(description: &str) -> Result<Response> {
    with_oauth_token_cache_headers(
        Response::from_json(&serde_json::json!({
            "error": "invalid_request",
            "error_description": description,
        }))?
        .with_status(400),
    )
}

fn with_oauth_token_cache_headers(mut response: Response) -> Result<Response> {
    response.headers_mut().set("Cache-Control", "no-store")?;
    response.headers_mut().set("Pragma", "no-cache")?;
    Ok(response)
}

#[cfg_attr(not(test), allow(dead_code))]
fn oauth_token_error_code(status: u16, error: &str) -> &'static str {
    match (status, error) {
        (401, "invalid_client") => "invalid_client",
        (400, "invalid_grant") => "invalid_grant",
        (400, "invalid_scope") => "invalid_scope",
        (400, "unsupported_grant_type") => "unsupported_grant_type",
        (400, "invalid_request") => "invalid_request",
        _ => "invalid_request",
    }
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
        return oauth_invalid_grant_response(
            "The provided authorization grant is invalid, expired, revoked, or does not match the redirection URI.",
        );
    };
    if !authorization_code_matches_request(&code_row, &app, &input.redirect_uri) {
        return oauth_invalid_grant_response(
            "The provided authorization grant is invalid, expired, revoked, or does not match the redirection URI.",
        );
    }
    if code_row.expires_at < now_unix_timestamp() {
        delete_oauth_authorization_code(db, &code_row.code).await?;
        return oauth_invalid_grant_response(
            "The provided authorization grant is invalid, expired, revoked, or does not match the redirection URI.",
        );
    }
    if !authorization_code_allows_client(&code_row, &input) {
        return oauth_invalid_grant_response(
            "The provided authorization grant is invalid, expired, revoked, or does not match the redirection URI.",
        );
    }

    let scopes = serde_json::from_str::<Vec<String>>(&code_row.scopes_json).unwrap_or_default();
    let access_token = issue_oauth_access_token(db, app.id, &code_row.account_id, &scopes).await?;
    crate::link_oauth_app_to_account(db, app.id, &code_row.account_id).await?;
    delete_oauth_authorization_code(db, &code_row.code).await?;
    if find_account_by_id(db, &code_row.account_id)
        .await?
        .is_none()
    {
        return oauth_invalid_grant_response(
            "The provided authorization grant is invalid, expired, revoked, or does not match the redirection URI.",
        );
    }
    with_oauth_token_cache_headers(Response::from_json(&build_oauth_token_document(
        &access_token.access_token,
        &scopes.join(" "),
    ))?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OAuthPostBodyKind {
    FormUrlencoded,
    Json,
}

fn classify_oauth_post_content_type(
    content_type: &str,
) -> std::result::Result<OAuthPostBodyKind, &'static str> {
    let content_type = content_type.trim().to_ascii_lowercase();
    if content_type.is_empty() {
        return Err("Content-Type header is required.");
    }
    if content_type.starts_with("application/json") {
        return Ok(OAuthPostBodyKind::Json);
    }
    if content_type.starts_with("application/x-www-form-urlencoded") {
        return Ok(OAuthPostBodyKind::FormUrlencoded);
    }
    Err("Content-Type must be application/x-www-form-urlencoded.")
}

async fn parse_oauth_token_request(
    req: &mut Request,
) -> std::result::Result<OAuthTokenRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default();

    match classify_oauth_post_content_type(&content_type) {
        Ok(OAuthPostBodyKind::Json) => req
            .json::<OAuthTokenRequest>()
            .await
            .map_err(|error| format!("invalid JSON token payload: {error}")),
        Ok(OAuthPostBodyKind::FormUrlencoded) => {
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
        Err(message) => Err(message.to_owned()),
    }
}

pub(in crate::oauth_apps) fn requested_oauth_token_scopes(value: Option<String>) -> Vec<String> {
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

pub(crate) async fn oauth_token_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let request = match parse_oauth_token_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return oauth_invalid_request_response(&message),
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
    let db = crate::D1Database::new(ctx.d1(&load_config(&ctx).database_binding)?);
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
    with_oauth_token_cache_headers(Response::from_json(
        &build_oauth_token_document_with_expires_in(
            &access_token,
            &requested_scopes.join(" "),
            APP_ACCESS_TOKEN_TTL_SECONDS,
        ),
    )?)
}

#[derive(Debug, Default, Deserialize)]
struct OAuthRevokeRequest {
    token: Option<String>,
    token_type_hint: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
}

pub(crate) async fn oauth_revoke_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let request = match parse_oauth_revoke_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return oauth_invalid_request_response(&message),
    };
    let Some(token) = trimmed_non_empty(request.token.as_deref()) else {
        return oauth_invalid_request_response("token is required");
    };
    let header_credentials = req
        .headers()
        .get("Authorization")?
        .as_deref()
        .and_then(parse_basic_authorization_header);
    let client_id = header_credentials
        .as_ref()
        .map(|(client_id, _)| client_id.clone())
        .or_else(|| trimmed_non_empty(request.client_id.as_deref()));
    let client_secret = header_credentials
        .map(|(_, client_secret)| client_secret)
        .or_else(|| trimmed_non_empty(request.client_secret.as_deref()));
    let (Some(client_id), Some(client_secret)) = (client_id, client_secret) else {
        return oauth_invalid_client_response();
    };
    let db = crate::D1Database::new(ctx.d1(&load_config(&ctx).database_binding)?);
    let Some(app) = find_oauth_app_by_client_id(&db, &client_id).await? else {
        return oauth_invalid_client_response();
    };
    if app.client_secret != client_secret {
        return oauth_invalid_client_response();
    }
    let _ = request.token_type_hint;
    revoke_oauth_access_token(&db, app.id, &token).await?;
    oauth_revoke_success_response()
}

fn oauth_revoke_success_response() -> Result<Response> {
    Ok(Response::empty()?.with_status(oauth_revoke_success_status()))
}

fn oauth_revoke_success_status() -> u16 {
    200
}

async fn parse_oauth_revoke_request(
    req: &mut Request,
) -> std::result::Result<OAuthRevokeRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default();

    match classify_oauth_post_content_type(&content_type) {
        Ok(OAuthPostBodyKind::Json) => req
            .json::<OAuthRevokeRequest>()
            .await
            .map_err(|error| format!("invalid JSON revoke payload: {error}")),
        Ok(OAuthPostBodyKind::FormUrlencoded) => {
            let form = req
                .form_data()
                .await
                .map_err(|error| format!("invalid form revoke payload: {error}"))?;
            Ok(OAuthRevokeRequest {
                token: form.get_field("token"),
                token_type_hint: form.get_field("token_type_hint"),
                client_id: form.get_field("client_id"),
                client_secret: form.get_field("client_secret"),
            })
        }
        Err(message) => Err(message.to_owned()),
    }
}

async fn revoke_oauth_access_token(db: &D1Database, oauth_app_id: i64, token: &str) -> Result<()> {
    let token_hash = oauth_bearer_token_hash(token);
    for sql in [
        "DELETE FROM oauth_access_tokens
         WHERE oauth_app_id = ?1
           AND (access_token_hash = ?2 OR access_token = ?3)",
        "DELETE FROM oauth_app_access_tokens
         WHERE oauth_app_id = ?1
           AND (access_token_hash = ?2 OR access_token = ?3)",
    ] {
        let bindings = [
            D1Type::Integer(i32::try_from(oauth_app_id).unwrap_or(i32::MAX)),
            D1Type::Text(token_hash.as_str()),
            D1Type::Text(token),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.run().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{OAuthAuthorizationCodeRow, pkce_code_challenge};
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
    fn oauth_revoke_success_is_empty_200() {
        assert_eq!(oauth_revoke_success_status(), 200);
    }

    #[test]
    fn classify_oauth_post_content_type_requires_form_or_json() {
        assert_eq!(
            classify_oauth_post_content_type("application/x-www-form-urlencoded"),
            Ok(OAuthPostBodyKind::FormUrlencoded)
        );
        assert_eq!(
            classify_oauth_post_content_type("application/x-www-form-urlencoded; charset=UTF-8"),
            Ok(OAuthPostBodyKind::FormUrlencoded)
        );
        assert_eq!(
            classify_oauth_post_content_type("application/json"),
            Ok(OAuthPostBodyKind::Json)
        );
        assert_eq!(
            classify_oauth_post_content_type(""),
            Err("Content-Type header is required.")
        );
        assert_eq!(
            classify_oauth_post_content_type("text/plain"),
            Err("Content-Type must be application/x-www-form-urlencoded.")
        );
    }

    #[test]
    fn invalid_grant_maps_to_http_400() {
        assert_eq!(
            oauth_token_error_code(400, "invalid_grant"),
            "invalid_grant"
        );
        assert_eq!(
            oauth_token_error_code(401, "invalid_client"),
            "invalid_client"
        );
        assert_eq!(oauth_invalid_grant_status(), 400);
        assert_eq!(oauth_invalid_client_status(), 401);
    }
}
