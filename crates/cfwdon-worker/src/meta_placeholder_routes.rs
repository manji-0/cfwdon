use crate::auth::find_account_by_email;
use crate::crypto_keys::generate_account_key_material;
use crate::{
    AccountReference, D1Database, LocalApiAuthentication, Request, Response, Result, RouteContext,
    actor_url, app_bearer_token_from_request, authenticate_local_api_request,
    build_app_verify_credentials_document_from_parts,
    build_app_verify_credentials_document_from_row, build_local_status_response,
    build_oauth_token_document, build_reject_follow_activity, build_relationship_for_target,
    build_remote_status_response, cache_public_response, can_view_local_status,
    delete_follow_by_target, delete_follower_by_actor, delete_remote_follow_request_by_actor,
    escape_html, find_account_by_id, find_account_by_username, find_authenticated_local_account,
    find_follower_follow_activity_id, find_local_status_by_object_uri,
    find_media_attachments_by_status_id, find_oauth_app_by_bearer_token,
    find_oauth_app_id_by_bearer_token, find_pending_remote_follow_request_by_actor,
    find_remote_actor_by_actor_uri, generate_entity_id, instance_base_url,
    is_public_activitypub_visibility, issue_oauth_access_token, load_account_stats, load_config,
    load_config_from_env, load_in_reply_to_account_id, local_status_ap_id, media_object_url,
    now_iso_string, oauth_access_token_has_any_scope, oauth_app_has_any_scope, oauth_app_scopes,
    parse_optional_bool, parse_relationship_query_ids, queue_remote_actor_activity_required,
    remote_account_rest_id, resolve_account_reference, resolve_status_reference,
    send_push_notification, store_account_password, store_account_private_key,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use worker::{Env, Fetch, Headers, Method, RequestInit, ResponseBody, d1::D1Type};

pub(crate) mod streaming;

pub(crate) use streaming::streaming_placeholder_response;

#[derive(Debug, Deserialize)]
struct OembedQuery {
    url: String,
    maxwidth: Option<u32>,
    maxheight: Option<u32>,
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnnualReportRow {
    account_id: String,
    year: i32,
    data_json: String,
    schema_version: i32,
    share_key: Option<String>,
    viewed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u64,
}

#[derive(Debug, Deserialize)]
struct StatusIdRow {
    id: String,
}

#[derive(Debug, Default, Deserialize)]
struct AccountRegistrationRequest {
    username: Option<String>,
    email: Option<String>,
    password: Option<String>,
    agreement: Option<String>,
    locale: Option<String>,
    reason: Option<String>,
}

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

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AccountRegistrationValidation {
    pub(crate) username: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) password_present: bool,
    pub(crate) agreement: Option<bool>,
}

fn oauth_base_url(config: &cfwdon_core::AppConfig) -> String {
    instance_base_url(config)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn normalized_registration_field(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalized_registration_username(value: Option<String>) -> Option<String> {
    normalized_registration_field(value).map(|value| value.to_ascii_lowercase())
}

fn normalized_registration_email(value: Option<String>) -> Option<String> {
    normalized_registration_field(value).map(|value| value.to_ascii_lowercase())
}

fn account_registration_composing(
    validation: &AccountRegistrationValidation,
) -> cfwdon_domain::ComposingRegistration {
    cfwdon_domain::ComposingRegistration {
        username: validation.username.clone(),
        email: validation.email.clone(),
        password_present: validation.password_present,
        agreement: validation.agreement,
    }
}

fn account_registration_api_details(
    validation: &AccountRegistrationValidation,
    uniqueness: cfwdon_domain::RegistrationUniquenessFacts,
) -> BTreeMap<&'static str, Vec<String>> {
    match cfwdon_domain::finalize_registration_validation(
        account_registration_composing(validation).validate(),
        uniqueness,
    ) {
        Ok(_) => BTreeMap::new(),
        Err(errors) => errors.into_api_details(),
    }
}

const OAUTH_SCOPES_SUPPORTED: &[&str] = &[
    "read",
    "profile",
    "write",
    "write:accounts",
    "write:blocks",
    "write:bookmarks",
    "write:collections",
    "write:conversations",
    "write:favourites",
    "write:filters",
    "write:follows",
    "write:lists",
    "write:media",
    "write:mutes",
    "write:notifications",
    "write:reports",
    "write:statuses",
    "read:accounts",
    "read:blocks",
    "read:bookmarks",
    "read:collections",
    "read:favourites",
    "read:filters",
    "read:follows",
    "read:lists",
    "read:mutes",
    "read:notifications",
    "read:search",
    "read:statuses",
    "follow",
    "push",
    "admin:read",
    "admin:read:accounts",
    "admin:read:reports",
    "admin:read:domain_allows",
    "admin:read:domain_blocks",
    "admin:read:ip_blocks",
    "admin:read:email_domain_blocks",
    "admin:read:canonical_email_blocks",
    "admin:write",
    "admin:write:accounts",
    "admin:write:reports",
    "admin:write:domain_allows",
    "admin:write:domain_blocks",
    "admin:write:ip_blocks",
    "admin:write:email_domain_blocks",
    "admin:write:canonical_email_blocks",
];

pub(crate) fn build_oauth_authorization_server_document(
    config: &cfwdon_core::AppConfig,
) -> serde_json::Value {
    let base_url = oauth_base_url(config);
    let issuer = format!("{}/", base_url.trim_end_matches('/'));
    serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{base_url}/oauth/authorize"),
        "token_endpoint": format!("{base_url}/oauth/token"),
        "userinfo_endpoint": format!("{base_url}/oauth/userinfo"),
        "revocation_endpoint": format!("{base_url}/oauth/revoke"),
        "app_registration_endpoint": format!("{base_url}/api/v1/apps"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query", "fragment", "form_post"],
        "grant_types_supported": ["authorization_code", "client_credentials"],
        "scopes_supported": OAUTH_SCOPES_SUPPORTED,
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "code_challenge_methods_supported": ["S256"],
        "service_documentation": "https://docs.joinmastodon.org/",
    })
}

pub(crate) fn build_oauth_userinfo_document(
    config: &cfwdon_core::AppConfig,
    account: &cfwdon_domain::LocalAccount,
) -> serde_json::Value {
    let base_url = oauth_base_url(config);
    let issuer = format!("{}/", base_url.trim_end_matches('/'));
    let actor = actor_url(config, account.username());
    let picture = account
        .avatar_object_key()
        .map(|object_key| serde_json::json!(media_object_url(config, object_key)))
        .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "iss": issuer,
        "sub": actor,
        "preferred_username": account.username(),
        "name": account.display_name(),
        "profile": format!("{base_url}/@{}", account.username()),
        "picture": picture,
    })
}

pub(crate) fn build_donation_campaign_document(
    config: &cfwdon_core::AppConfig,
) -> Option<serde_json::Value> {
    let value =
        serde_json::from_str::<serde_json::Value>(config.donation_campaign_json.as_deref()?)
            .ok()?;
    value.as_object()?;
    Some(value)
}

fn build_oembed_html(
    account: &cfwdon_domain::LocalAccount,
    status_url: &str,
    content_html: &str,
) -> String {
    format!(
        concat!(
            "<blockquote class=\"mastodon-embed\" ",
            "style=\"background:#FCF8FF;border-radius:8px;border:1px solid #C9C4DA;",
            "margin:0;max-width:540px;min-width:270px;overflow:hidden;padding:24px;\">",
            "<div style=\"color:#1C1A25;font-family:system-ui,-apple-system,BlinkMacSystemFont,",
            "'Segoe UI',Oxygen,Ubuntu,Cantarell,'Fira Sans','Droid Sans','Helvetica Neue',Roboto,sans-serif;",
            "font-size:14px;letter-spacing:0.25px;line-height:20px;\">",
            "{content_html}",
            "</div>",
            "<div style=\"color:#787588;font-family:system-ui,-apple-system,BlinkMacSystemFont,",
            "'Segoe UI',Oxygen,Ubuntu,Cantarell,'Fira Sans','Droid Sans','Helvetica Neue',Roboto,sans-serif;",
            "font-size:14px;letter-spacing:0.25px;line-height:20px;margin-top:16px;\">",
            "Post by @{username}",
            "</div>",
            "<a href=\"{status_url}\" ",
            "style=\"align-items:center;color:#1C1A25;display:flex;flex-direction:column;",
            "font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',Oxygen,Ubuntu,",
            "Cantarell,'Fira Sans','Droid Sans','Helvetica Neue',Roboto,sans-serif;font-size:14px;",
            "font-weight:500;justify-content:center;letter-spacing:0.25px;line-height:20px;",
            "margin-top:16px;text-decoration:none;\">",
            "View on cfwdon",
            "</a>",
            "</blockquote>"
        ),
        content_html = content_html,
        username = escape_html(account.username()),
        status_url = escape_html(status_url),
    )
}

const OEMBED_DEFAULT_WIDTH: u32 = 400;
const OEMBED_DEFAULT_HEIGHT: u32 = 200;

fn oembed_capped_dimension(default: u32, requested_max: Option<u32>) -> u32 {
    match requested_max {
        Some(max) => default.min(max),
        None => default,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OembedFormatDecision {
    Json,
    UnsupportedXml,
    Unrecognized,
}

fn resolve_oembed_format(format: Option<&str>) -> OembedFormatDecision {
    match format.map(|value| value.trim().to_ascii_lowercase()) {
        None => OembedFormatDecision::Json,
        Some(ref value) if value.is_empty() || value == "json" => OembedFormatDecision::Json,
        Some(ref value) if value == "xml" => OembedFormatDecision::UnsupportedXml,
        Some(_) => OembedFormatDecision::Unrecognized,
    }
}

fn build_oembed_document(
    config: &cfwdon_core::AppConfig,
    account: &cfwdon_domain::LocalAccount,
    status_url: &str,
    content_html: &str,
    author_name: &str,
    maxwidth: Option<u32>,
    maxheight: Option<u32>,
) -> serde_json::Value {
    serde_json::json!({
        "type": "rich",
        "version": "1.0",
        "title": format!("New status by {}", account.username()),
        "author_name": author_name,
        "author_url": actor_url(config, account.username()),
        "provider_name": config.instance_domain,
        "provider_url": format!("{}/", oauth_base_url(config)),
        "cache_age": 86400,
        "html": build_oembed_html(account, status_url, content_html),
        "width": oembed_capped_dimension(OEMBED_DEFAULT_WIDTH, maxwidth),
        "height": oembed_capped_dimension(OEMBED_DEFAULT_HEIGHT, maxheight),
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_app_verify_credentials_document(
    config: &cfwdon_core::AppConfig,
) -> serde_json::Value {
    build_app_verify_credentials_document_from_parts(
        "0",
        &config.instance_name,
        None,
        &[String::from("read")],
        &[String::from("urn:ietf:wg:oauth:2.0:oob")],
        "urn:ietf:wg:oauth:2.0:oob",
        config.web_push_vapid_public_key.as_deref().unwrap_or(""),
    )
}

fn current_campaign_year() -> i32 {
    js_sys::Date::new_0().get_utc_full_year() as i32
}

fn annual_report_bounds(year: i32) -> (String, String) {
    (
        format!("{year:04}-01-01T00:00:00Z"),
        format!("{:04}-01-01T00:00:00Z", year + 1),
    )
}

fn annual_report_document(row: &AnnualReportRow) -> serde_json::Value {
    let data = serde_json::from_str::<serde_json::Value>(&row.data_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    serde_json::json!({
        "year": row.year,
        "data": data,
        "schema_version": row.schema_version,
        "share_url": row
            .share_key
            .as_ref()
            .map(|value| format!("/annual_reports/{}/{}", row.year, value))
            .unwrap_or_default(),
        "account_id": row.account_id,
    })
}

async fn list_generated_annual_reports(
    db: &crate::D1Database,
    account_id: &str,
    pending_only: bool,
) -> Result<Vec<AnnualReportRow>> {
    let sql = if pending_only {
        "SELECT account_id, year, data_json, schema_version, share_key, viewed_at
         FROM generated_annual_reports
         WHERE account_id = ?1
           AND viewed_at IS NULL
         ORDER BY year DESC"
    } else {
        "SELECT account_id, year, data_json, schema_version, share_key, viewed_at
         FROM generated_annual_reports
         WHERE account_id = ?1
         ORDER BY year DESC"
    };

    db.prepare(sql)
        .bind_refs(&D1Type::Text(account_id))?
        .all()
        .await
        .and_then(|__d1| crate::d1_results::<AnnualReportRow>(&__d1))
}

async fn find_generated_annual_report(
    db: &crate::D1Database,
    account_id: &str,
    year: i32,
) -> Result<Option<AnnualReportRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(year)];
    db.prepare(
        "SELECT account_id, year, data_json, schema_version, share_key, viewed_at
         FROM generated_annual_reports
         WHERE account_id = ?1
           AND year = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<AnnualReportRow>(None)
    .await
}

async fn count_account_statuses_between(
    db: &crate::D1Database,
    account_id: &str,
    start: &str,
    end: &str,
) -> Result<u64> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(start),
        D1Type::Text(end),
    ];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM statuses
             WHERE account_id = ?1
               AND datetime(created_at) >= datetime(?2)
               AND datetime(created_at) < datetime(?3)",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|value| value.count).unwrap_or(0))
}

async fn list_recent_public_status_ids_between(
    db: &crate::D1Database,
    account_id: &str,
    start: &str,
    end: &str,
    limit: u32,
) -> Result<Vec<String>> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(start),
        D1Type::Text(end),
        D1Type::Integer(limit as i32),
    ];
    Ok(db
        .prepare(
            "SELECT id
             FROM statuses
             WHERE account_id = ?1
               AND datetime(created_at) >= datetime(?2)
               AND datetime(created_at) < datetime(?3)
               AND visibility IN ('public', 'unlisted')
             ORDER BY created_at DESC, id DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await
        .and_then(|__d1| crate::d1_results::<StatusIdRow>(&__d1))?
        .into_iter()
        .map(|row| row.id)
        .collect())
}

async fn create_generated_annual_report(
    db: &crate::D1Database,
    account: &cfwdon_domain::LocalAccount,
    year: i32,
) -> Result<AnnualReportRow> {
    let (start, end) = annual_report_bounds(year);
    let stats = load_account_stats(db, account.id()).await?;
    let posts_count = count_account_statuses_between(db, account.id(), &start, &end).await?;
    let top_statuses =
        list_recent_public_status_ids_between(db, account.id(), &start, &end, 3).await?;
    let share_key = generate_entity_id(12)?;
    let data_json = serde_json::json!({
        "display_name": if account.display_name().trim().is_empty() {
            account.username().to_owned()
        } else {
            account.display_name().to_owned()
        },
        "username": account.username(),
        "joined_at": crate::timestamp_to_mastodon_iso8601(account.created_at()),
        "posts_count": posts_count,
        "followers_count": stats.followers_count,
        "following_count": stats.following_count,
        "top_statuses": {
            "first": top_statuses.first().cloned(),
            "second": top_statuses.get(1).cloned(),
            "third": top_statuses.get(2).cloned(),
        },
    });
    let data_json_string = serde_json::to_string(&data_json).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize annual report data: {error}"))
    })?;
    let now = now_iso_string()?;
    let bindings = [
        D1Type::Text(account.id()),
        D1Type::Integer(year),
        D1Type::Text(data_json_string.as_str()),
        D1Type::Text(share_key.as_str()),
        D1Type::Text(now.as_str()),
        D1Type::Text(now.as_str()),
    ];
    db.prepare(
        "INSERT OR REPLACE INTO generated_annual_reports (
            account_id,
            year,
            data_json,
            schema_version,
            share_key,
            viewed_at,
            created_at,
            updated_at
         ) VALUES (?1, ?2, ?3, 1, ?4, NULL, ?5, ?6)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    find_generated_annual_report(db, account.id(), year)
        .await?
        .ok_or_else(|| {
            worker::Error::RustError("generated annual report was not persisted".to_owned())
        })
}

async fn mark_generated_annual_report_viewed(
    db: &crate::D1Database,
    account_id: &str,
    year: i32,
) -> Result<()> {
    let now = now_iso_string()?;
    let bindings = [
        D1Type::Text(now.as_str()),
        D1Type::Text(account_id),
        D1Type::Integer(year),
    ];
    db.prepare(
        "UPDATE generated_annual_reports
         SET viewed_at = COALESCE(viewed_at, ?1),
             updated_at = ?1
         WHERE account_id = ?2
           AND year = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn is_authenticated_request(
    req: &Request,
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<bool> {
    Ok(find_authenticated_local_account(req, db, config)
        .await?
        .is_some())
}

fn invalid_access_token_response() -> Result<Response> {
    let mut response = Response::from_json(&serde_json::json!({
        "error": "The access token is invalid",
    }))?
    .with_status(401);
    response
        .headers_mut()
        .set("WWW-Authenticate", r#"Bearer error="invalid_token""#)?;
    Ok(response)
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

fn outside_authorized_scopes_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is outside the authorized scopes",
    }))?
    .with_status(403))
}

fn validation_failed_response(details: BTreeMap<&'static str, Vec<String>>) -> Result<Response> {
    let mut messages = Vec::new();
    for (field, field_errors) in &details {
        let label = match *field {
            "username" => "Username",
            "email" => "Email",
            "password" => "Password",
            "agreement" => "Agreement",
            _ => field,
        };
        for error in field_errors {
            messages.push(format!("{label} {error}"));
        }
    }
    Ok(Response::from_json(&serde_json::json!({
        "error": format!("Validation failed: {}", messages.join(", ")),
        "details": details,
    }))?
    .with_status(422))
}

async fn parse_account_registration_request(
    req: &mut Request,
) -> std::result::Result<AccountRegistrationRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<AccountRegistrationRequest>()
            .await
            .map_err(|error| format!("invalid JSON account registration payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form account registration payload: {error}"))?;
        AccountRegistrationRequest {
            username: form.get_field("username"),
            email: form.get_field("email"),
            password: form.get_field("password"),
            agreement: form.get_field("agreement"),
            locale: form.get_field("locale"),
            reason: form.get_field("reason"),
        }
    };

    request.username = normalized_registration_username(request.username);
    request.email = normalized_registration_email(request.email);
    request.password = normalized_registration_field(request.password);
    request.locale = normalized_registration_field(request.locale);
    request.reason = normalized_registration_field(request.reason);
    request.agreement = request
        .agreement
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    Ok(request)
}

async fn insert_registered_account(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    username: &str,
    email: &str,
) -> Result<String> {
    let id = generate_entity_id(16)?;
    let display_name = username.to_owned();
    let key_material = generate_account_key_material().await?;
    let bindings = [
        D1Type::Text(id.as_str()),
        D1Type::Text(username),
        D1Type::Text(email),
        D1Type::Text(display_name.as_str()),
        D1Type::Text(""),
        D1Type::Text(key_material.public_key_pem.as_str()),
    ];
    db.prepare(
        "INSERT INTO accounts (
            id,
            username,
            access_email,
            display_name,
            fields_json,
            discoverable,
            default_quote_policy,
            private_key_jwk,
            public_key_pem,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            '[]',
            0,
            'public',
            ?5,
            ?6,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    store_account_private_key(db, config, &id, &key_material.private_key_jwk).await?;

    for admin_email in &config.admin_emails {
        if let Some(admin) = find_account_by_email(db, admin_email).await? {
            let _ = send_push_notification(
                db,
                config,
                admin.id(),
                "admin.sign_up",
                serde_json::json!({
                    "account_id": id,
                    "username": username,
                    "email": email,
                }),
            )
            .await;
        }
    }
    Ok(id)
}

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

async fn upsert_pending_email_confirmation(
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

async fn update_pending_email_confirmation_sent_at(
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
        oauth_base_url(config),
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

async fn send_email_confirmation_message(
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

pub(crate) async fn oauth_authorization_server_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    oauth_authorization_server_response_for_config(&config)
}

pub(crate) fn oauth_authorization_server_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    oauth_authorization_server_response_for_config(&config)
}

fn oauth_authorization_server_response_for_config(config: &crate::AppConfig) -> Result<Response> {
    cache_public_response(
        Response::from_json(&build_oauth_authorization_server_document(config))?,
        crate::CACHE_TTL_OAUTH_DISCOVERY,
    )
}

pub(crate) async fn oauth_userinfo_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match authenticate_local_api_request(&req, &db, &config).await? {
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["profile"]) {
                return outside_authorized_scopes_response();
            }
            auth.account
        }
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
            return invalid_access_token_response();
        }
        LocalApiAuthentication::Auth0(account) => account,
        LocalApiAuthentication::None => return invalid_access_token_response(),
    };
    Response::from_json(&build_oauth_userinfo_document(&config, &account))
}

pub(crate) async fn oembed_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: OembedQuery = req.query()?;
    match resolve_oembed_format(query.format.as_deref()) {
        OembedFormatDecision::Json => {}
        OembedFormatDecision::UnsupportedXml => {
            return Response::error("Not Implemented", 501);
        }
        OembedFormatDecision::Unrecognized => {
            return Response::error("Bad Request", 400);
        }
    }
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(status) = find_local_status_by_object_uri(&db, &config, &query.url).await? else {
        return Response::error("Record not found", 404);
    };
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Response::error("Record not found", 404);
    }
    let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
        return Response::error("Record not found", 404);
    };

    let status_url = local_status_ap_id(&config, &account, &status);
    let author_name = if account.display_name().trim().is_empty() {
        account.username().to_owned()
    } else {
        account.display_name().to_owned()
    };

    cache_public_response(
        Response::from_json(&build_oembed_document(
            &config,
            &account,
            &status_url,
            &status.content_html,
            &author_name,
            query.maxwidth,
            query.maxheight,
        ))?,
        crate::CACHE_TTL_OEMBED,
    )
}

pub(crate) async fn donation_campaigns_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    if !is_authenticated_request(&req, &db, &config).await? {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "This method requires an authenticated user",
        }))?
        .with_status(422));
    }
    let Some(document) = build_donation_campaign_document(&config) else {
        return Ok(Response::empty()?.with_status(204));
    };
    Ok(Response::from_json(&document)?.with_status(200))
}

pub(crate) async fn annual_reports_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let reports = list_generated_annual_reports(&db, account.id(), true).await?;
    let response = reports
        .iter()
        .map(annual_report_document)
        .collect::<Vec<_>>();
    Response::from_json(&response)
}

pub(crate) async fn annual_report_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::error("annual report not found", 404);
    };
    let Some(report) = find_generated_annual_report(&db, account.id(), year).await? else {
        return Response::error("annual report not found", 404);
    };
    Response::from_json(&annual_report_document(&report))
}

pub(crate) async fn annual_report_action_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::empty();
    };
    let url = req.url()?;

    if url.path().ends_with("/read") {
        mark_generated_annual_report_viewed(&db, account.id(), year).await?;
        return Response::empty();
    }

    if year != current_campaign_year() {
        return Response::empty();
    }
    if find_generated_annual_report(&db, account.id(), year)
        .await?
        .is_some()
    {
        return Response::empty();
    }

    create_generated_annual_report(&db, &account, year).await?;
    Ok(Response::empty()?.with_status(202))
}

pub(crate) async fn annual_report_state_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::from_json(&serde_json::json!({
            "state": "unavailable",
            "available": false,
        }));
    };
    if let Some(report) = find_generated_annual_report(&db, account.id(), year).await? {
        return Response::from_json(&serde_json::json!({
            "state": if report.viewed_at.is_some() { "viewed" } else { "ready" },
            "available": true,
        }));
    }

    Response::from_json(&serde_json::json!({
        "state": if year == current_campaign_year() { "not_generated" } else { "unavailable" },
        "available": false,
    }))
}

pub(crate) async fn app_verify_credentials_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(token) = app_bearer_token_from_request(&req)? else {
        return Response::error("The access token is invalid", 401);
    };
    let Some(app) = find_oauth_app_by_bearer_token(&db, &token).await? else {
        return Response::error("The access token is invalid", 401);
    };
    Response::from_json(&build_app_verify_credentials_document_from_row(
        &app, &config,
    ))
}

pub(crate) async fn create_email_confirmation_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
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
    let db = crate::bind_request_d1(&ctx, &config)?;
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
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(account) = find_authenticated_local_account(&req, &db, &config).await? else {
        return invalid_access_token_response();
    };
    let confirmed = find_pending_email_confirmation(&db, account.id())
        .await?
        .is_none()
        && !account.access_email().trim().is_empty();
    Response::from_json(&confirmed)
}

pub(crate) async fn statuses_index_placeholder_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let mut response = Vec::new();

    for status_id in parse_relationship_query_ids(&req)? {
        let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
            continue;
        };

        match status {
            crate::ResolvedStatus::Local(status) => {
                let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                    continue;
                };
                if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
                    continue;
                }

                let media = find_media_attachments_by_status_id(&db, &status.id).await?;
                let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
                response.push(
                    build_local_status_response(
                        &db,
                        &config,
                        viewer.as_ref(),
                        &status,
                        &account,
                        in_reply_to_account_id,
                        media,
                    )
                    .await?,
                );
            }
            crate::ResolvedStatus::Remote(status) => {
                if !is_public_activitypub_visibility(status.visibility.as_str()) {
                    continue;
                }
                let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await?
                else {
                    continue;
                };
                response.push(
                    build_remote_status_response(&db, &config, viewer.as_ref(), &status, &actor)
                        .await?,
                );
            }
        }
    }

    Response::from_json(&response)
}

pub(crate) async fn accounts_index_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let mut response = Vec::new();

    for account_id in parse_relationship_query_ids(&req)? {
        let fetch_context =
            crate::RemoteCollectionFetchContext::public(&config, &db, viewer.as_ref());
        match crate::resolve_account_reference_with_fetch(&db, &account_id, Some(&fetch_context))
            .await?
        {
            Some(AccountReference::Local(account)) => {
                let stats = load_account_stats(&db, account.id()).await?;
                response.push(crate::MastodonAccountResponse::from_account_with_stats(
                    &account, &config, &stats,
                ));
            }
            Some(AccountReference::Remote(actor)) => {
                let fetched = crate::fetch_remote_actor_profile_with_context(
                    &actor.actor_uri,
                    Some(&fetch_context),
                )
                .await;
                let mut account = match fetched.as_ref() {
                    Ok(fetched) => {
                        if let Err(error) = crate::upsert_remote_actor(&db, &fetched.profile).await
                        {
                            crate::log_json_event(serde_json::json!({
                                "event": "remote_actor_upsert_failed",
                                "actor_uri": fetched.profile.actor_uri,
                                "error": error.to_string(),
                            }));
                        }
                        match crate::find_remote_actor_by_actor_uri(&db, &fetched.profile.actor_uri)
                            .await?
                        {
                            Some(cached) => {
                                crate::MastodonAccountResponse::from_remote_actor(&cached)
                            }
                            None => crate::MastodonAccountResponse::from_remote_actor_profile(
                                &fetched.profile,
                            ),
                        }
                    }
                    Err(error) => {
                        crate::log_json_event(serde_json::json!({
                            "event": "remote_actor_refresh_failed",
                            "actor_uri": actor.actor_uri,
                            "error": error.to_string(),
                        }));
                        crate::MastodonAccountResponse::from_remote_actor(&actor)
                    }
                };
                if let Ok(fetched) = fetched.as_ref() {
                    let social_counts_updated_at =
                        crate::find_remote_actor_by_actor_uri(&db, &fetched.profile.actor_uri)
                            .await?
                            .and_then(|row| row.social_counts_updated_at);
                    crate::enrich_remote_account_response(
                        &db,
                        &fetched.profile.actor_uri,
                        social_counts_updated_at.as_deref(),
                        &mut account,
                        &fetched.document,
                        Some(&fetch_context),
                    )
                    .await?;
                } else if let Err(error) = crate::reconcile_remote_account_status_summary(
                    &db,
                    &actor.actor_uri,
                    &mut account,
                )
                .await
                {
                    crate::log_json_event(serde_json::json!({
                        "event": "remote_account_enrichment_failed",
                        "actor_uri": actor.actor_uri,
                        "stage": "status_summary",
                        "error": error.to_string(),
                    }));
                }
                response.push(account);
            }
            None => {}
        }
    }

    Response::from_json(&response)
}

pub(crate) async fn create_account_placeholder_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(token) = app_bearer_token_from_request(&req)? else {
        return invalid_access_token_response();
    };
    let Some(app) = find_oauth_app_by_bearer_token(&db, &token).await? else {
        return invalid_access_token_response();
    };
    if !oauth_app_has_any_scope(&app, &["write:accounts", "write"]) {
        return outside_authorized_scopes_response();
    }

    let request = match parse_account_registration_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(&message, 422),
    };
    let agreement = parse_optional_bool(request.agreement.as_deref()).unwrap_or_default();
    let validation = AccountRegistrationValidation {
        username: request.username.clone(),
        email: request.email.clone(),
        password_present: request.password.is_some(),
        agreement,
    };
    let uniqueness = cfwdon_domain::RegistrationUniquenessFacts {
        username_taken: if let Some(username) = request.username.as_deref() {
            find_account_by_username(&db, username).await?.is_some()
        } else {
            false
        },
        email_taken: if let Some(email) = request.email.as_deref() {
            find_account_by_email(&db, email).await?.is_some()
        } else {
            false
        },
    };
    let details = account_registration_api_details(&validation, uniqueness);
    if !details.is_empty() {
        return validation_failed_response(details);
    }
    let account_id = insert_registered_account(
        &db,
        &config,
        request
            .username
            .as_deref()
            .expect("validated username presence"),
        request.email.as_deref().expect("validated email presence"),
    )
    .await?;
    store_account_password(
        &db,
        &account_id,
        request
            .password
            .as_deref()
            .expect("validated password presence"),
    )
    .await?;
    let app_id = find_oauth_app_id_by_bearer_token(&db, &token)
        .await?
        .expect("loaded app must have an id");
    link_oauth_app_to_account(&db, app_id, &account_id).await?;
    let confirmation_token = generate_entity_id(32)?;
    let pending_email = request.email.as_deref().expect("validated email presence");
    upsert_pending_email_confirmation(&db, &account_id, app_id, pending_email, &confirmation_token)
        .await?;
    if send_email_confirmation_message(&ctx, &config, pending_email, &confirmation_token).await? {
        update_pending_email_confirmation_sent_at(&db, &account_id, &confirmation_token).await?;
    }
    let app_scopes = oauth_app_scopes(&app);
    let access_token = issue_oauth_access_token(&db, app_id, &account_id, &app_scopes).await?;

    Response::from_json(&build_oauth_token_document(
        &access_token.access_token,
        &app_scopes.join(" "),
    ))
}

pub(crate) async fn remove_from_followers_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;

    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, target.username());
            delete_follow_by_target(&db, target.id(), &actor_url(&config, viewer.username()))
                .await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &viewer,
                target.id(),
                &target_actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            let accepted_follow_activity_id = find_follower_follow_activity_id(
                &db,
                viewer.id(),
                &actor.actor_uri,
                &actor.actor_uri,
            )
            .await?;
            if let Some(request) =
                find_pending_remote_follow_request_by_actor(&db, viewer.id(), &actor.actor_uri)
                    .await?
            {
                delete_remote_follow_request_by_actor(
                    &db,
                    viewer.id(),
                    &actor.actor_uri,
                    &actor.actor_uri,
                )
                .await?;
                if let Some(follow_activity_id) = request.follow_activity_id.as_deref() {
                    let payload = build_reject_follow_activity(
                        &config,
                        &viewer,
                        follow_activity_id,
                        &actor.actor_uri,
                    )?;
                    let _ = queue_remote_actor_activity_required(
                        &db,
                        viewer.id(),
                        &actor.actor_uri,
                        &payload,
                    )
                    .await;
                }
            }
            if let Some(follow_activity_id) = accepted_follow_activity_id.as_deref() {
                let payload = build_reject_follow_activity(
                    &config,
                    &viewer,
                    follow_activity_id,
                    &actor.actor_uri,
                )?;
                let _ = queue_remote_actor_activity_required(
                    &db,
                    viewer.id(),
                    &actor.actor_uri,
                    &payload,
                )
                .await;
            }
            delete_follower_by_actor(&db, viewer.id(), &actor.actor_uri, &actor.actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &viewer,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annual_report_bounds_cover_exact_calendar_year() {
        assert_eq!(
            annual_report_bounds(2025),
            (
                "2025-01-01T00:00:00Z".to_owned(),
                "2026-01-01T00:00:00Z".to_owned()
            )
        );
    }

    #[test]
    fn oembed_height_defaults_to_integer_when_maxheight_absent() {
        let config = cfwdon_core::AppConfig::new("https://social.example", "cfwdon", "test");
        let account = cfwdon_domain::LocalAccount::from_record(
            cfwdon_domain::LocalAccountRecord::test_fixture("acct-1", "alice"),
        );
        let document = build_oembed_document(
            &config,
            &account,
            "https://social.example/@alice/statuses/1",
            "<p>hi</p>",
            "alice",
            None,
            None,
        );
        assert_eq!(document["height"], serde_json::json!(OEMBED_DEFAULT_HEIGHT));
        assert!(document["height"].is_number());
        assert_eq!(document["width"], serde_json::json!(OEMBED_DEFAULT_WIDTH));
    }

    #[test]
    fn oembed_dimensions_respect_maxwidth_and_maxheight_caps() {
        assert_eq!(oembed_capped_dimension(400, Some(200)), 200);
        assert_eq!(oembed_capped_dimension(400, Some(800)), 400);
        assert_eq!(oembed_capped_dimension(200, Some(50)), 50);
        assert_eq!(oembed_capped_dimension(200, None), 200);
    }

    #[test]
    fn oembed_format_json_and_absent_are_accepted() {
        assert_eq!(resolve_oembed_format(None), OembedFormatDecision::Json);
        assert_eq!(
            resolve_oembed_format(Some("json")),
            OembedFormatDecision::Json
        );
        assert_eq!(
            resolve_oembed_format(Some("JSON")),
            OembedFormatDecision::Json
        );
    }

    #[test]
    fn oembed_format_xml_is_not_implemented() {
        assert_eq!(
            resolve_oembed_format(Some("xml")),
            OembedFormatDecision::UnsupportedXml
        );
    }

    #[test]
    fn oembed_format_unrecognized_is_bad_request() {
        assert_eq!(
            resolve_oembed_format(Some("yaml")),
            OembedFormatDecision::Unrecognized
        );
    }

    #[test]
    fn build_oembed_html_escapes_username_and_status_url() {
        let account = cfwdon_domain::LocalAccount::from_record(
            cfwdon_domain::LocalAccountRecord::test_fixture("acct-1", "alice\"onclick=x"),
        );
        let html = build_oembed_html(
            &account,
            "https://evil.example/\" onmouseover=\"alert(1)",
            "<p>safe already-escaped content</p>",
        );
        assert!(html.contains("Post by @alice&quot;onclick=x"));
        assert!(html.contains("href=\"https://evil.example/&quot; onmouseover=&quot;alert(1)\""));
        assert!(html.contains("<p>safe already-escaped content</p>"));
        assert!(!html.contains("Post by @alice\"onclick=x"));
    }
}
