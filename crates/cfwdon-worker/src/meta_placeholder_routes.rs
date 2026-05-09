use crate::crypto_keys::generate_account_key_material;
use crate::{
    AccountReference, LocalApiAuthentication, NotificationsQuery, Request, Response, Result,
    RouteContext, TimelinePaginationQuery, actor_url, app_bearer_token_from_request,
    authenticate_local_api_request, build_announcements_document,
    build_app_verify_credentials_document_from_parts,
    build_app_verify_credentials_document_from_row, build_delete_quote_authorization_activity,
    build_local_status_response, build_oauth_token_document, build_reject_follow_activity,
    build_relationship_for_target, build_remote_status_response, build_timeline_link_header,
    can_view_local_status, clear_local_status_quote, collect_visible_notifications,
    delete_follow_by_target, delete_follower_by_actor, delete_remote_follow_request_by_actor,
    enqueue_status_update_activity, enqueue_targeted_outbox_activity, extract_hashtags_from_html,
    extract_hashtags_from_text, filter_notification_entries_by_query, find_account_by_email,
    find_account_by_id, find_account_by_username, find_authenticated_local_account,
    find_conversation_for_account, find_conversation_id_by_status_id,
    find_follower_follow_activity_id, find_local_status_by_object_uri,
    find_media_attachments_by_status_id, find_oauth_access_token_by_bearer_token,
    find_oauth_app_by_bearer_token, find_oauth_app_id_by_bearer_token,
    find_pending_remote_follow_request_by_actor, find_remote_actor_by_actor_uri,
    find_remote_status_by_id, find_status_by_id, generate_entity_id, insert_status_edit_snapshot,
    instance_base_url, is_local_status_thread_muted_by, is_muted_actor,
    is_public_activitypub_visibility, issue_oauth_access_token, list_announcement_read_ids,
    list_followed_tag_names, list_follower_delivery_targets, list_local_direct_timeline_statuses,
    list_local_home_timeline_statuses, list_local_public_statuses_by_tag,
    list_local_public_timeline_statuses, list_membership_refs,
    list_membership_variants_for_local_account, list_membership_variants_for_remote_actor,
    list_remote_home_timeline_statuses, list_remote_public_statuses_by_tag,
    list_remote_public_timeline_statuses, list_row_by_id, load_account_stats,
    load_announcement_reaction_state, load_config, load_in_reply_to_account_id,
    load_latest_filter_updated_at, load_remote_status_updated_at, load_status_updated_at,
    local_status_ap_id, local_status_target_uri, matches_tag_timeline_filters, media_object_url,
    normalize_status_history_entry, now_iso_string, oauth_access_token_has_any_scope,
    oauth_app_has_any_scope, oauth_app_scopes, parse_optional_bool, parse_relationship_query_ids,
    queue_remote_actor_activity, queue_remote_actor_activity_required, remote_account_rest_id,
    remote_status_has_active_quote, remote_status_has_media, resolve_account_reference,
    resolve_status_reference, resolve_timeline_cursor, send_push_notification,
    status_has_active_quote, store_account_password, timeline_fetch_limit, timeline_limit,
    update_remote_status_quote_state,
};
use async_stream::try_stream;
use futures_util::{StreamExt, pin_mut};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use worker::{
    D1Database, Fetch, Headers, Method, RequestInit, ResponseBody, WebSocketPair, d1::D1Type,
};

#[derive(Debug, Deserialize)]
struct OembedQuery {
    url: String,
    maxwidth: Option<u32>,
    maxheight: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct QuotesQuery {
    #[serde(flatten)]
    pagination: TimelinePaginationQuery,
}

#[derive(Debug, Default, Deserialize)]
struct StreamingQuery {
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    access_token: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamingChannelValidationError {
    UnknownChannelRequested,
    MissingTag,
    MissingList,
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

pub(crate) fn validate_account_registration_request(
    validation: &AccountRegistrationValidation,
) -> BTreeMap<&'static str, Vec<String>> {
    let mut details = BTreeMap::new();

    match validation.username.as_deref() {
        None => {
            details.insert("username", vec!["can't be blank".to_owned()]);
        }
        Some(username)
            if !username
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_') =>
        {
            details.insert(
                "username",
                vec!["must contain only letters, numbers and underscores".to_owned()],
            );
        }
        _ => {}
    }

    match validation.email.as_deref() {
        None => {
            details.insert("email", vec!["can't be blank".to_owned()]);
        }
        Some(email) if !email.contains('@') => {
            details.insert("email", vec!["is invalid".to_owned()]);
        }
        _ => {}
    }

    if !validation.password_present {
        details.insert("password", vec!["can't be blank".to_owned()]);
    }

    if validation.agreement != Some(true) {
        details.insert("agreement", vec!["must be accepted".to_owned()]);
    }

    details
}

pub(crate) fn normalize_streaming_channel(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn streaming_channel_requires_tag(stream: &str) -> bool {
    matches!(stream, "hashtag" | "hashtag:local")
}

fn streaming_channel_requires_list(stream: &str) -> bool {
    stream == "list"
}

pub(crate) fn streaming_channel_requires_auth(stream: &str) -> bool {
    matches!(stream, "user" | "user:notification" | "list" | "direct")
}

pub(crate) fn validate_streaming_channel_request(
    stream: Option<&str>,
    tag: Option<&str>,
    list: Option<&str>,
    extra_path: Option<&str>,
) -> std::result::Result<String, StreamingChannelValidationError> {
    if extra_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return Err(StreamingChannelValidationError::UnknownChannelRequested);
    }
    let Some(stream) = normalize_streaming_channel(stream) else {
        return Err(StreamingChannelValidationError::UnknownChannelRequested);
    };
    if !matches!(
        stream.as_str(),
        "public"
            | "public:media"
            | "public:local"
            | "public:local:media"
            | "public:remote"
            | "public:remote:media"
            | "hashtag"
            | "hashtag:local"
            | "user"
            | "user:notification"
            | "list"
            | "direct"
    ) {
        return Err(StreamingChannelValidationError::UnknownChannelRequested);
    }
    if streaming_channel_requires_tag(&stream)
        && tag
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(StreamingChannelValidationError::MissingTag);
    }
    if streaming_channel_requires_list(&stream)
        && list
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(StreamingChannelValidationError::MissingList);
    }
    Ok(stream)
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
    let actor = actor_url(config, &account.username);
    let picture = account
        .avatar_object_key
        .as_deref()
        .map(|object_key| serde_json::json!(media_object_url(config, object_key)))
        .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "iss": issuer,
        "sub": actor,
        "preferred_username": account.username,
        "name": account.display_name,
        "profile": format!("{base_url}/@{}", account.username),
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
        username = account.username,
        status_url = status_url,
    )
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
    db: &worker::D1Database,
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
        .await?
        .results::<AnnualReportRow>()
}

async fn find_generated_annual_report(
    db: &worker::D1Database,
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
    db: &worker::D1Database,
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
    db: &worker::D1Database,
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
        .await?
        .results::<StatusIdRow>()?
        .into_iter()
        .map(|row| row.id)
        .collect())
}

async fn create_generated_annual_report(
    db: &worker::D1Database,
    account: &cfwdon_domain::LocalAccount,
    year: i32,
) -> Result<AnnualReportRow> {
    let (start, end) = annual_report_bounds(year);
    let stats = load_account_stats(db, &account.id).await?;
    let posts_count = count_account_statuses_between(db, &account.id, &start, &end).await?;
    let top_statuses =
        list_recent_public_status_ids_between(db, &account.id, &start, &end, 3).await?;
    let share_key = generate_entity_id(12)?;
    let data_json = serde_json::json!({
        "display_name": if account.display_name.trim().is_empty() {
            account.username.clone()
        } else {
            account.display_name.clone()
        },
        "username": account.username,
        "joined_at": account.created_at,
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
        D1Type::Text(account.id.as_str()),
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

    find_generated_annual_report(db, &account.id, year)
        .await?
        .ok_or_else(|| {
            worker::Error::RustError("generated annual report was not persisted".to_owned())
        })
}

async fn mark_generated_annual_report_viewed(
    db: &worker::D1Database,
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
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<bool> {
    Ok(find_authenticated_local_account(req, db, config)
        .await?
        .is_some())
}

fn invalid_access_token_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "The access token is invalid",
    }))?
    .with_status(401))
}

fn streaming_bad_request_response(error: StreamingChannelValidationError) -> Result<Response> {
    let message = match error {
        StreamingChannelValidationError::UnknownChannelRequested => "Unknown channel requested",
        StreamingChannelValidationError::MissingTag => "Missing tag parameter",
        StreamingChannelValidationError::MissingList => "Missing list parameter",
    };
    Ok(Response::from_json(&serde_json::json!({
        "error": message,
    }))?
    .with_status(400))
}

fn websocket_protocol_access_token(req: &Request) -> Result<Option<String>> {
    let Some(protocols) = req.headers().get("Sec-WebSocket-Protocol")? else {
        return Ok(None);
    };

    Ok(protocols
        .split(',')
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned))
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
        D1Type::Text(key_material.private_key_jwk.as_str()),
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

    for admin_email in &config.admin_emails {
        if let Some(admin) = find_account_by_email(db, admin_email).await? {
            let _ = send_push_notification(
                db,
                config,
                &admin.id,
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
    Response::from_json(&build_oauth_authorization_server_document(&config))
}

pub(crate) async fn oauth_userinfo_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
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
        LocalApiAuthentication::Access(account) => account,
        LocalApiAuthentication::None => return invalid_access_token_response(),
    };
    Response::from_json(&build_oauth_userinfo_document(&config, &account))
}

pub(crate) async fn oembed_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: OembedQuery = req.query()?;
    let db = ctx.d1(&config.database_binding)?;
    let Some(status) = find_local_status_by_object_uri(&db, &config, &query.url).await? else {
        return Response::error("Record not found", 404);
    };
    if !is_public_activitypub_visibility(&status.visibility) {
        return Response::error("Record not found", 404);
    }
    let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
        return Response::error("Record not found", 404);
    };

    let status_url = local_status_ap_id(&config, &account, &status);
    let author_name = if account.display_name.trim().is_empty() {
        account.username.clone()
    } else {
        account.display_name.clone()
    };

    Response::from_json(&serde_json::json!({
        "type": "rich",
        "version": "1.0",
        "title": format!("New status by {}", account.username),
        "author_name": author_name,
        "author_url": actor_url(&config, &account.username),
        "provider_name": config.instance_domain,
        "provider_url": format!("{}/", oauth_base_url(&config)),
        "cache_age": 86400,
        "html": build_oembed_html(&account, &status_url, &status.content_html),
        "width": query.maxwidth.unwrap_or(400),
        "height": query.maxheight,
    }))
}

pub(crate) async fn donation_campaigns_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
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
    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let reports = list_generated_annual_reports(&db, &account.id, true).await?;
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
    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::error("annual report not found", 404);
    };
    let Some(report) = find_generated_annual_report(&db, &account.id, year).await? else {
        return Response::error("annual report not found", 404);
    };
    Response::from_json(&annual_report_document(&report))
}

pub(crate) async fn annual_report_action_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::empty();
    };
    let url = req.url()?;

    if url.path().ends_with("/read") {
        mark_generated_annual_report_viewed(&db, &account.id, year).await?;
        return Response::empty();
    }

    if year != current_campaign_year() {
        return Response::empty();
    }
    if find_generated_annual_report(&db, &account.id, year)
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
    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::from_json(&serde_json::json!({
            "state": "unavailable",
            "available": false,
        }));
    };
    if let Some(report) = find_generated_annual_report(&db, &account.id, year).await? {
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
    let db = ctx.d1(&config.database_binding)?;
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
    let db = ctx.d1(&config.database_binding)?;
    let (account, request_oauth_app_id) =
        match authenticate_local_api_request(&req, &db, &config).await? {
            LocalApiAuthentication::OAuthToken(auth) => {
                if !oauth_access_token_has_any_scope(&auth.token, &["write:accounts", "write"]) {
                    return outside_authorized_scopes_response();
                }
                (auth.account, Some(auth.token.oauth_app_id))
            }
            LocalApiAuthentication::Access(account) => (account, None),
            LocalApiAuthentication::AppToken
            | LocalApiAuthentication::InvalidBearer
            | LocalApiAuthentication::None => {
                return invalid_access_token_response();
            }
        };
    let Some(pending) = find_pending_email_confirmation(&db, &account.id).await? else {
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
        && existing.id != account.id
    {
        let mut details = BTreeMap::new();
        details.insert("email", vec!["has already been taken".to_owned()]);
        return validation_failed_response(details);
    }
    upsert_pending_email_confirmation(
        &db,
        &account.id,
        pending.oauth_app_id,
        pending_email,
        &confirmation_token,
    )
    .await?;
    if send_email_confirmation_message(&ctx, &config, pending_email, &confirmation_token).await? {
        update_pending_email_confirmation_sent_at(&db, &account.id, &confirmation_token).await?;
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
    let db = ctx.d1(&config.database_binding)?;
    let Some(pending) = find_pending_email_confirmation_by_token(&db, token).await? else {
        return email_confirmation_html_response(
            "Confirmation token is invalid",
            "The confirmation link is invalid or has already been used.",
            404,
        );
    };
    if let Some(existing) = find_account_by_email(&db, &pending.pending_email).await?
        && existing.id != pending.account_id
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
    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_authenticated_local_account(&req, &db, &config).await? else {
        return invalid_access_token_response();
    };
    let confirmed = find_pending_email_confirmation(&db, &account.id)
        .await?
        .is_none()
        && !account.access_email.trim().is_empty();
    Response::from_json(&confirmed)
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct StreamingEvent {
    created_at: String,
    id: String,
    event: &'static str,
    data: String,
}

#[derive(Debug)]
struct StreamingBatch {
    events: Vec<StreamingEvent>,
    tracked_status_ids: Vec<String>,
    last_id: Option<String>,
}

fn sse_comment_bytes(value: &str) -> Vec<u8> {
    format!(": {value}\n\n").into_bytes()
}

fn sse_event_bytes(event: &StreamingEvent) -> Vec<u8> {
    format!("event: {}\ndata: {}\n\n", event.event, event.data).into_bytes()
}

fn announcement_reaction_entries_for_id(
    state: &HashMap<(String, String), (u64, bool)>,
    announcement_id: &str,
) -> BTreeMap<String, (u64, bool)> {
    state
        .iter()
        .filter(|((id, _), _)| id == announcement_id)
        .map(|((_, name), value)| (name.clone(), *value))
        .collect()
}

async fn streaming_notification_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let query = NotificationsQuery {
        since_id: since_id.map(str::to_owned),
        limit: Some(40),
        ..NotificationsQuery::default()
    };
    let entries = collect_visible_notifications(db, config, viewer, &query, 160).await?;
    let filtered = filter_notification_entries_by_query(entries, &query);
    let last_id = filtered.first().map(|entry| entry.id.clone());
    let mut events = Vec::with_capacity(filtered.len());

    for entry in filtered.into_iter().rev() {
        events.push(StreamingEvent {
            created_at: entry.created_at,
            id: entry.id,
            event: "notification",
            data: serde_json::to_string(&entry.value).map_err(|error| {
                worker::Error::RustError(format!(
                    "failed to serialize notification stream payload: {error}"
                ))
            })?,
        });
    }

    Ok(StreamingBatch {
        events,
        tracked_status_ids: Vec::new(),
        last_id,
    })
}

async fn streaming_public_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    stream: &str,
    tag: Option<&str>,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let include_local = matches!(
        stream,
        "public"
            | "public:media"
            | "public:local"
            | "public:local:media"
            | "hashtag"
            | "hashtag:local"
    ) || stream.starts_with("hashtag");
    let include_remote = matches!(
        stream,
        "public" | "public:media" | "public:remote" | "public:remote:media" | "hashtag"
    );
    let only_media = stream.ends_with(":media");
    let cursor = resolve_timeline_cursor(
        db,
        &TimelinePaginationQuery {
            since_id: since_id.map(str::to_owned),
            limit: Some(40),
            ..TimelinePaginationQuery::default()
        },
    )
    .await?;
    let query_limit = timeline_fetch_limit(40);
    let mut entries = Vec::new();
    let mut tracked_status_ids = Vec::new();

    if stream.starts_with("hashtag") {
        let Some(tag) = tag else {
            return Ok(StreamingBatch {
                events: Vec::new(),
                tracked_status_ids: Vec::new(),
                last_id: None,
            });
        };
        if include_local {
            for status in list_local_public_statuses_by_tag(db, tag, &cursor, query_limit).await? {
                let status_tags = extract_hashtags_from_text(&status._text_content);
                if !matches_tag_timeline_filters(
                    &status_tags,
                    tag,
                    &crate::TagTimelineQuery::default(),
                ) {
                    continue;
                }
                let Some(account) = find_account_by_id(db, &status.account_id).await? else {
                    continue;
                };
                if let Some(viewer) = viewer
                    && is_local_status_thread_muted_by(db, &viewer.id, &status).await?
                {
                    continue;
                }
                let media = find_media_attachments_by_status_id(db, &status.id).await?;
                if only_media && media.is_empty() {
                    continue;
                }
                entries.push((
                    status.created_at.clone(),
                    status.id.clone(),
                    serde_json::to_string(
                        &build_local_status_response(
                            db,
                            config,
                            viewer,
                            &status,
                            &account,
                            load_in_reply_to_account_id(db, &status).await?,
                            media,
                        )
                        .await?,
                    )
                    .map_err(|error| {
                        worker::Error::RustError(format!(
                            "failed to serialize hashtag stream payload: {error}"
                        ))
                    })?,
                ));
                tracked_status_ids.push(status.id.clone());
            }
        }
        if include_remote {
            for (status, actor) in
                list_remote_public_statuses_by_tag(db, tag, &cursor, query_limit).await?
            {
                let status_tags = extract_hashtags_from_html(&status.content_html);
                if !matches_tag_timeline_filters(
                    &status_tags,
                    tag,
                    &crate::TagTimelineQuery::default(),
                ) {
                    continue;
                }
                if only_media && !remote_status_has_media(db, &status.id).await? {
                    continue;
                }
                if let Some(viewer) = viewer
                    && is_muted_actor(db, &viewer.id, &actor.actor_uri).await?
                {
                    continue;
                }
                entries.push((
                    status.published_at.clone(),
                    status.id.clone(),
                    serde_json::to_string(
                        &build_remote_status_response(db, config, viewer, &status, &actor).await?,
                    )
                    .map_err(|error| {
                        worker::Error::RustError(format!(
                            "failed to serialize hashtag stream payload: {error}"
                        ))
                    })?,
                ));
                tracked_status_ids.push(status.id.clone());
            }
        }
    } else {
        if include_local {
            for status in list_local_public_timeline_statuses(db, &cursor, query_limit).await? {
                let Some(account) = find_account_by_id(db, &status.account_id).await? else {
                    continue;
                };
                if let Some(viewer) = viewer
                    && is_local_status_thread_muted_by(db, &viewer.id, &status).await?
                {
                    continue;
                }
                let media = find_media_attachments_by_status_id(db, &status.id).await?;
                if only_media && media.is_empty() {
                    continue;
                }
                entries.push((
                    status.created_at.clone(),
                    status.id.clone(),
                    serde_json::to_string(
                        &build_local_status_response(
                            db, config, viewer, &status, &account, None, media,
                        )
                        .await?,
                    )
                    .map_err(|error| {
                        worker::Error::RustError(format!(
                            "failed to serialize public stream payload: {error}"
                        ))
                    })?,
                ));
                tracked_status_ids.push(status.id.clone());
            }
        }
        if include_remote {
            for (status, actor) in
                list_remote_public_timeline_statuses(db, &cursor, query_limit).await?
            {
                if only_media && !remote_status_has_media(db, &status.id).await? {
                    continue;
                }
                if let Some(viewer) = viewer
                    && is_muted_actor(db, &viewer.id, &actor.actor_uri).await?
                {
                    continue;
                }
                entries.push((
                    status.published_at.clone(),
                    status.id.clone(),
                    serde_json::to_string(
                        &build_remote_status_response(db, config, viewer, &status, &actor).await?,
                    )
                    .map_err(|error| {
                        worker::Error::RustError(format!(
                            "failed to serialize public stream payload: {error}"
                        ))
                    })?,
                ));
                tracked_status_ids.push(status.id.clone());
            }
        }
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let last_id = entries.last().map(|(_, id, _)| id.clone());
    let events = entries
        .into_iter()
        .map(|(created_at, id, data)| StreamingEvent {
            created_at,
            id,
            event: "conversation",
            data,
        })
        .collect::<Vec<_>>();

    Ok(StreamingBatch {
        events,
        tracked_status_ids,
        last_id,
    })
}

async fn streaming_home_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let cursor = resolve_timeline_cursor(
        db,
        &TimelinePaginationQuery {
            since_id: since_id.map(str::to_owned),
            limit: Some(40),
            ..TimelinePaginationQuery::default()
        },
    )
    .await?;
    let query_limit = timeline_fetch_limit(40);
    let mut entries = Vec::new();
    let mut tracked_status_ids = Vec::new();
    let mut seen_status_ids = HashSet::new();

    for status in list_local_home_timeline_statuses(db, &viewer.id, &cursor, query_limit).await? {
        if !seen_status_ids.insert(status.id.clone()) {
            continue;
        }
        let Some(account) = find_account_by_id(db, &status.account_id).await? else {
            continue;
        };
        if is_muted_actor(db, &viewer.id, &actor_url(config, &account.username)).await? {
            continue;
        }
        if is_local_status_thread_muted_by(db, &viewer.id, &status).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            status.id.clone(),
            serde_json::to_string(
                &build_local_status_response(
                    db,
                    config,
                    Some(viewer),
                    &status,
                    &account,
                    load_in_reply_to_account_id(db, &status).await?,
                    media,
                )
                .await?,
            )
            .map_err(|error| {
                worker::Error::RustError(format!(
                    "failed to serialize home stream payload: {error}"
                ))
            })?,
        ));
        tracked_status_ids.push(status.id.clone());
    }

    for (status, actor) in
        list_remote_home_timeline_statuses(db, &viewer.id, &cursor, query_limit).await?
    {
        if !seen_status_ids.insert(status.id.clone()) {
            continue;
        }
        if is_muted_actor(db, &viewer.id, &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            status.id.clone(),
            serde_json::to_string(
                &build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
            )
            .map_err(|error| {
                worker::Error::RustError(format!(
                    "failed to serialize home stream payload: {error}"
                ))
            })?,
        ));
        tracked_status_ids.push(status.id.clone());
    }

    for tag in list_followed_tag_names(db, &viewer.id).await? {
        for status in list_local_public_statuses_by_tag(db, &tag, &cursor, query_limit).await? {
            if !seen_status_ids.insert(status.id.clone()) {
                continue;
            }
            let Some(account) = find_account_by_id(db, &status.account_id).await? else {
                continue;
            };
            if is_muted_actor(db, &viewer.id, &actor_url(config, &account.username)).await? {
                continue;
            }
            if is_local_status_thread_muted_by(db, &viewer.id, &status).await? {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            entries.push((
                status.created_at.clone(),
                status.id.clone(),
                serde_json::to_string(
                    &build_local_status_response(
                        db,
                        config,
                        Some(viewer),
                        &status,
                        &account,
                        load_in_reply_to_account_id(db, &status).await?,
                        media,
                    )
                    .await?,
                )
                .map_err(|error| {
                    worker::Error::RustError(format!(
                        "failed to serialize home stream payload: {error}"
                    ))
                })?,
            ));
            tracked_status_ids.push(status.id.clone());
        }

        for (status, actor) in
            list_remote_public_statuses_by_tag(db, &tag, &cursor, query_limit).await?
        {
            if !seen_status_ids.insert(status.id.clone()) {
                continue;
            }
            if is_muted_actor(db, &viewer.id, &actor.actor_uri).await? {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                status.id.clone(),
                serde_json::to_string(
                    &build_remote_status_response(db, config, Some(viewer), &status, &actor)
                        .await?,
                )
                .map_err(|error| {
                    worker::Error::RustError(format!(
                        "failed to serialize home stream payload: {error}"
                    ))
                })?,
            ));
            tracked_status_ids.push(status.id.clone());
        }
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let last_id = entries.last().map(|(_, id, _)| id.clone());
    let events = entries
        .into_iter()
        .map(|(created_at, id, data)| StreamingEvent {
            created_at,
            id,
            event: "update",
            data,
        })
        .collect::<Vec<_>>();

    Ok(StreamingBatch {
        events,
        tracked_status_ids,
        last_id,
    })
}

async fn streaming_direct_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let cursor = resolve_timeline_cursor(
        db,
        &TimelinePaginationQuery {
            since_id: since_id.map(str::to_owned),
            limit: Some(40),
            ..TimelinePaginationQuery::default()
        },
    )
    .await?;
    let query_limit = timeline_fetch_limit(40);
    let mut entries = Vec::new();
    let mut tracked_conversation_ids = Vec::new();
    let mut seen_conversation_ids = HashSet::new();

    for status in list_local_direct_timeline_statuses(db, &viewer.id, &cursor, query_limit).await? {
        let Some(conversation_id) = find_conversation_id_by_status_id(db, &status.id).await? else {
            continue;
        };
        if !seen_conversation_ids.insert(conversation_id.clone()) {
            continue;
        }
        let Some(conversation) =
            find_conversation_for_account(db, &viewer.id, &conversation_id).await?
        else {
            continue;
        };
        entries.push((
            status.created_at.clone(),
            conversation.id.clone(),
            serde_json::to_string(
                &crate::conversation_document(db, config, viewer, &conversation).await?,
            )
            .map_err(|error| {
                worker::Error::RustError(format!(
                    "failed to serialize direct stream payload: {error}"
                ))
            })?,
        ));
        tracked_conversation_ids.push(conversation.id.clone());
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let last_id = entries.last().map(|(_, id, _)| id.clone());
    let events = entries
        .into_iter()
        .map(|(created_at, id, data)| StreamingEvent {
            created_at,
            id,
            event: "update",
            data,
        })
        .collect::<Vec<_>>();

    Ok(StreamingBatch {
        events,
        tracked_status_ids: tracked_conversation_ids,
        last_id,
    })
}

async fn streaming_list_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    list_id: &str,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let cursor = resolve_timeline_cursor(
        db,
        &TimelinePaginationQuery {
            since_id: since_id.map(str::to_owned),
            limit: Some(40),
            ..TimelinePaginationQuery::default()
        },
    )
    .await?;
    let query_limit = timeline_fetch_limit(40);
    let Some(list) = list_row_by_id(db, &viewer.id, list_id).await? else {
        return Ok(StreamingBatch {
            events: Vec::new(),
            tracked_status_ids: Vec::new(),
            last_id: None,
        });
    };
    let membership_refs = list_membership_refs(db, list_id)
        .await?
        .into_iter()
        .map(|row| row.target_account_ref)
        .collect::<HashSet<_>>();
    let mut entries = Vec::new();
    let mut tracked_status_ids = Vec::new();

    for status in list_local_public_timeline_statuses(db, &cursor, query_limit).await? {
        let Some(author) = find_account_by_id(db, &status.account_id).await? else {
            continue;
        };
        if !list_membership_variants_for_local_account(&author, config)
            .into_iter()
            .any(|candidate| membership_refs.contains(&candidate))
        {
            continue;
        }
        if list.replies_policy == "none" && status.in_reply_to_id.is_some() {
            continue;
        }
        if is_local_status_thread_muted_by(db, &viewer.id, &status).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            status.id.clone(),
            serde_json::to_string(
                &build_local_status_response(
                    db,
                    config,
                    Some(viewer),
                    &status,
                    &author,
                    load_in_reply_to_account_id(db, &status).await?,
                    media,
                )
                .await?,
            )
            .map_err(|error| {
                worker::Error::RustError(format!(
                    "failed to serialize list stream payload: {error}"
                ))
            })?,
        ));
        tracked_status_ids.push(status.id.clone());
    }

    for (status, actor) in list_remote_public_timeline_statuses(db, &cursor, query_limit).await? {
        if !list_membership_variants_for_remote_actor(&actor)
            .into_iter()
            .any(|candidate| membership_refs.contains(&candidate))
        {
            continue;
        }
        if list.replies_policy == "none" && status.in_reply_to_uri.is_some() {
            continue;
        }
        if is_muted_actor(db, &viewer.id, &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            status.id.clone(),
            serde_json::to_string(
                &build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
            )
            .map_err(|error| {
                worker::Error::RustError(format!(
                    "failed to serialize list stream payload: {error}"
                ))
            })?,
        ));
        tracked_status_ids.push(status.id.clone());
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let last_id = entries.last().map(|(_, id, _)| id.clone());
    let events = entries
        .into_iter()
        .map(|(created_at, id, data)| StreamingEvent {
            created_at,
            id,
            event: "update",
            data,
        })
        .collect::<Vec<_>>();

    Ok(StreamingBatch {
        events,
        tracked_status_ids,
        last_id,
    })
}

async fn streaming_status_delta_events(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    tracked_status_ids: &[String],
    deleted_status_ids: &mut HashSet<String>,
    updated_status_ids: &mut HashSet<String>,
) -> Result<Vec<StreamingEvent>> {
    let mut events = Vec::new();

    for status_id in tracked_status_ids.iter().rev().take(200) {
        if deleted_status_ids.contains(status_id) || updated_status_ids.contains(status_id) {
            continue;
        }

        if let Some(status) = find_status_by_id(db, status_id).await? {
            let Some(updated_at) = load_status_updated_at(db, &status.id).await? else {
                continue;
            };
            if updated_at == status.created_at {
                continue;
            }
            let Some(account) = find_account_by_id(db, &status.account_id).await? else {
                continue;
            };
            if let Some(viewer) = viewer
                && is_local_status_thread_muted_by(db, &viewer.id, &status).await?
            {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let payload = build_local_status_response(
                db,
                config,
                viewer,
                &status,
                &account,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?;
            events.push(StreamingEvent {
                created_at: updated_at,
                id: status.id.clone(),
                event: "status.update",
                data: serde_json::to_string(&payload).map_err(|error| {
                    worker::Error::RustError(format!(
                        "failed to serialize streaming local status update payload: {error}"
                    ))
                })?,
            });
            updated_status_ids.insert(status.id.clone());
            continue;
        }

        if let Some(status) = find_remote_status_by_id(db, status_id).await? {
            let Some(updated_at) = load_remote_status_updated_at(db, &status.id).await? else {
                continue;
            };
            if updated_at == status.published_at {
                continue;
            }
            if let Some(viewer) = viewer
                && is_muted_actor(db, &viewer.id, &status.actor_uri).await?
            {
                continue;
            }
            let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
                continue;
            };
            let payload = build_remote_status_response(db, config, viewer, &status, &actor).await?;
            events.push(StreamingEvent {
                created_at: updated_at,
                id: status.id.clone(),
                event: "status.update",
                data: serde_json::to_string(&payload).map_err(|error| {
                    worker::Error::RustError(format!(
                        "failed to serialize streaming remote status update payload: {error}"
                    ))
                })?,
            });
            updated_status_ids.insert(status.id.clone());
            continue;
        }

        deleted_status_ids.insert(status_id.clone());
        events.push(StreamingEvent {
            created_at: now_iso_string()?,
            id: status_id.clone(),
            event: "delete",
            data: status_id.clone(),
        });
    }

    Ok(events)
}

fn build_streaming_event_stream(
    db: D1Database,
    config: cfwdon_core::AppConfig,
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
    viewer: Option<crate::LocalAccount>,
) -> impl futures_util::TryStream<
    Ok = Vec<u8>,
    Error = worker::Error,
    Item = std::result::Result<Vec<u8>, worker::Error>,
> + 'static {
    try_stream! {
        yield sse_comment_bytes(&format!("stream={stream_name}"));
        let mut since_id = None::<String>;
        let mut tracked_status_ids = Vec::<String>::new();
        let mut tracked_status_id_set = HashSet::<String>::new();
        let mut deleted_status_ids = HashSet::<String>::new();
        let mut updated_status_ids = HashSet::<String>::new();
        let mut emitted_event_ids = HashSet::<String>::new();
        let mut last_filter_updated_at = None::<String>;
        let mut last_announcements = HashMap::<String, String>::new();
        let mut last_announcement_reactions = HashMap::<(String, String), (u64, bool)>::new();
        let mut initialized = false;
        loop {
            let is_initial_poll = !initialized;
            let batch = if stream_name == "user" {
                let viewer = viewer.as_ref().ok_or_else(|| worker::Error::RustError(
                    "missing authenticated viewer for user stream".to_owned()
                ))?;
                streaming_home_batch(&db, &config, viewer, since_id.as_deref()).await?
            } else if stream_name == "user:notification" {
                let viewer = viewer.as_ref().ok_or_else(|| worker::Error::RustError(
                    "missing authenticated viewer for notification stream".to_owned()
                ))?;
                streaming_notification_batch(&db, &config, viewer, since_id.as_deref()).await?
            } else if stream_name == "list" {
                let viewer = viewer.as_ref().ok_or_else(|| worker::Error::RustError(
                    "missing authenticated viewer for list stream".to_owned()
                ))?;
                let list_id = list
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| worker::Error::RustError(
                        "missing list id for list stream".to_owned()
                    ))?;
                streaming_list_batch(&db, &config, viewer, list_id, since_id.as_deref()).await?
            } else if stream_name == "direct" {
                let viewer = viewer.as_ref().ok_or_else(|| worker::Error::RustError(
                    "missing authenticated viewer for direct stream".to_owned()
                ))?;
                streaming_direct_batch(&db, &config, viewer, since_id.as_deref()).await?
            } else {
                streaming_public_batch(
                    &db,
                    &config,
                    viewer.as_ref(),
                    &stream_name,
                    tag.as_deref(),
                    since_id.as_deref(),
                )
                .await?
            };
            if let Some(next_since_id) = batch.last_id.clone() {
                since_id = Some(next_since_id);
            }
            for status_id in batch.tracked_status_ids {
                if tracked_status_id_set.insert(status_id.clone()) {
                    tracked_status_ids.push(status_id);
                }
            }
            while tracked_status_ids.len() > 200 {
                let removed = tracked_status_ids.remove(0);
                tracked_status_id_set.remove(&removed);
            }
            let mut events = if is_initial_poll {
                for event in &batch.events {
                    emitted_event_ids.insert(format!("{}:{}", event.event, event.id));
                }
                Vec::new()
            } else {
                batch.events
            };
            if !is_initial_poll && stream_name != "user:notification" {
                let delta_events = streaming_status_delta_events(
                    &db,
                    &config,
                    viewer.as_ref(),
                    &tracked_status_ids,
                    &mut deleted_status_ids,
                    &mut updated_status_ids,
                )
                .await?;
                events.extend(delta_events);
            }
            if stream_name == "user" {
                let viewer = viewer.as_ref().ok_or_else(|| worker::Error::RustError(
                    "missing authenticated viewer for user stream".to_owned()
                ))?;
                let current_filter_updated_at =
                    load_latest_filter_updated_at(&db, &viewer.id).await?;
                if let Some(current_filter_updated_at) = current_filter_updated_at {
                    let changed = last_filter_updated_at
                        .as_deref()
                        .map(|value| value != current_filter_updated_at.as_str())
                        .unwrap_or(false);
                    if !is_initial_poll && changed {
                        events.push(StreamingEvent {
                            created_at: current_filter_updated_at.clone(),
                            id: current_filter_updated_at.clone(),
                            event: "filters_changed",
                            data: "undefined".to_owned(),
                        });
                    }
                    last_filter_updated_at = Some(current_filter_updated_at);
                }
                let read_ids = list_announcement_read_ids(&db, &viewer.id).await?;
                let reaction_state = load_announcement_reaction_state(&db, &viewer.id).await?;
                let announcements =
                    build_announcements_document(&config, &read_ids, &reaction_state);
                let mut current_announcements = HashMap::<String, String>::new();
                for announcement in announcements {
                    let Some(id) = announcement
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                    else {
                        continue;
                    };
                    let payload = serde_json::to_string(&announcement).map_err(|error| {
                        worker::Error::RustError(format!(
                            "failed to serialize announcement stream payload: {error}"
                        ))
                    })?;
                    let current_reactions = announcement_reaction_entries_for_id(&reaction_state, &id);
                    let previous_reactions = announcement_reaction_entries_for_id(&last_announcement_reactions, &id);
                    if !is_initial_poll && current_reactions != previous_reactions {
                        for (name, (count, _me)) in &current_reactions {
                            let previous = last_announcement_reactions
                                .get(&(id.clone(), name.clone()))
                                .copied();
                            if previous != Some((*count, *_me)) {
                                events.push(StreamingEvent {
                                    created_at: announcement
                                        .get("published_at")
                                        .and_then(serde_json::Value::as_str)
                                        .or_else(|| announcement.get("updated_at").and_then(serde_json::Value::as_str))
                                        .unwrap_or_default()
                                        .to_owned(),
                                    id: format!("{id}:{name}"),
                                    event: "announcement.reaction",
                                    data: serde_json::json!({
                                        "name": name,
                                        "count": count,
                                        "announcement_id": id,
                                    })
                                    .to_string(),
                                });
                            }
                        }
                    } else if !is_initial_poll && last_announcements.get(&id) != Some(&payload) {
                        events.push(StreamingEvent {
                            created_at: announcement
                                .get("published_at")
                                .and_then(serde_json::Value::as_str)
                                .or_else(|| announcement.get("updated_at").and_then(serde_json::Value::as_str))
                                .unwrap_or_default()
                                .to_owned(),
                            id: id.clone(),
                            event: "announcement",
                            data: payload.clone(),
                        });
                    }
                    current_announcements.insert(id, payload);
                }
                for removed_id in last_announcements
                    .keys()
                    .filter(|id| !current_announcements.contains_key(*id))
                    .cloned()
                    .collect::<Vec<_>>()
                {
                    if !is_initial_poll {
                        events.push(StreamingEvent {
                            created_at: now_iso_string()?,
                            id: removed_id.clone(),
                            event: "announcement.delete",
                            data: removed_id,
                        });
                    }
                }
                last_announcement_reactions = reaction_state;
                last_announcements = current_announcements;
            }
            initialized = true;
            events.retain(|event| emitted_event_ids.insert(format!("{}:{}", event.event, event.id)));
            if events.is_empty() {
                yield sse_comment_bytes("thump");
            } else {
                for event in events {
                    yield sse_event_bytes(&event);
                }
            }
            worker::Delay::from(Duration::from_secs(3)).await;
        }
    }
}

pub(crate) async fn streaming_placeholder_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let query: StreamingQuery = req.query().unwrap_or_default();
    let wants_websocket = req
        .headers()
        .get("Upgrade")
        .ok()
        .flatten()
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let stream_query = if wants_websocket && query.stream.is_none() {
        Some("user")
    } else {
        query.stream.as_deref()
    };
    let extra_path = ctx
        .param("any")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let stream = match validate_streaming_channel_request(
        stream_query,
        query.tag.as_deref(),
        query.list.as_deref(),
        extra_path,
    ) {
        Ok(stream) => stream,
        Err(error) => return streaming_bad_request_response(error),
    };
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let authenticated = match authenticate_local_api_request(&req, &db, &config).await? {
        LocalApiAuthentication::Access(viewer) => Some(viewer),
        LocalApiAuthentication::OAuthToken(auth) => Some(auth.account),
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
            return invalid_access_token_response();
        }
        LocalApiAuthentication::None => {
            let token = query
                .access_token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .or(websocket_protocol_access_token(&req)?);
            match token {
                Some(token) => {
                    let Some(access_token) =
                        find_oauth_access_token_by_bearer_token(&db, &token).await?
                    else {
                        return invalid_access_token_response();
                    };
                    if !oauth_access_token_has_any_scope(
                        &access_token,
                        &["read", "read:statuses", "read:notifications"],
                    ) {
                        return invalid_access_token_response();
                    }
                    find_account_by_id(&db, &access_token.account_id).await?
                }
                None => None,
            }
        }
    };

    if streaming_channel_requires_auth(&stream) && authenticated.is_none() {
        return invalid_access_token_response();
    }

    if wants_websocket {
        let pair = WebSocketPair::new()?;
        pair.server.accept()?;
        let websocket = pair.server.clone();
        let db_for_ws = db;
        let config_for_ws = config.clone();
        let stream_name = stream.clone();
        let tag_for_ws = query.tag.clone();
        let list_for_ws = query.list.clone();
        let viewer_for_ws = authenticated.clone();
        spawn_local(async move {
            let event_stream = build_streaming_event_stream(
                db_for_ws,
                config_for_ws,
                stream_name,
                tag_for_ws,
                list_for_ws,
                viewer_for_ws,
            );
            pin_mut!(event_stream);
            while let Some(Ok(bytes)) = event_stream.next().await {
                if let Ok(text) = std::str::from_utf8(&bytes)
                    && websocket.send_with_str(text).is_err()
                {
                    break;
                }
            }
            let _ = websocket.close(Some(1000), Some("stream closed"));
        });
        return Response::from_websocket(pair.client);
    }

    if matches!(
        stream.as_str(),
        "public"
            | "public:media"
            | "public:local"
            | "public:local:media"
            | "public:remote"
            | "public:remote:media"
            | "hashtag"
            | "hashtag:local"
            | "user"
            | "user:notification"
            | "list"
            | "direct"
    ) {
        let stream_body = build_streaming_event_stream(
            db,
            config,
            stream.clone(),
            query.tag.clone(),
            query.list.clone(),
            authenticated.clone(),
        );
        let mut response = Response::from_stream(stream_body)?;
        response
            .headers_mut()
            .set("Content-Type", "text/event-stream")?;
        response.headers_mut().set("Cache-Control", "no-cache")?;
        Ok(response)
    } else {
        let mut body = format!(": cfwdon-placeholder stream={stream}\n");
        if let Some(tag) = query
            .tag
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            body.push_str(&format!(": tag={tag}\n"));
        }
        if let Some(list) = query
            .list
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            body.push_str(&format!(": list={list}\n"));
        }
        body.push('\n');
        let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
        response
            .headers_mut()
            .set("Content-Type", "text/event-stream")?;
        response.headers_mut().set("Cache-Control", "no-cache")?;
        Ok(response)
    }
}

pub(crate) async fn statuses_index_placeholder_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
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
                if !is_public_activitypub_visibility(&status.visibility) {
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

pub(crate) async fn status_quotes_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: QuotesQuery = req.query().unwrap_or_default();
    let limit = timeline_limit(&query.pagination);
    let query_limit = timeline_fetch_limit(limit);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let cursor = resolve_timeline_cursor(&db, &query.pagination).await?;

    let Some(status_id) = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Response::error("missing status id route parameter", 400);
    };

    let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
        return Response::error("status not found", 404);
    };

    let target_uri = match status {
        crate::ResolvedStatus::Local(status) => {
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                return Response::error("status not found", 404);
            };
            if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
                return Response::error("status not found", 404);
            }
            local_status_target_uri(&status)
        }
        crate::ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            status.object_uri
        }
    };

    let mut quotes: Vec<(String, String, serde_json::Value)> = Vec::new();

    for quote in list_local_status_quotes_by_uri(&db, &target_uri, &cursor, query_limit).await? {
        let Some(account) = find_account_by_id(&db, &quote.account_id).await? else {
            continue;
        };
        if !can_view_local_status(&db, &quote, viewer.as_ref(), &account).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(&db, &quote.id).await?;
        let in_reply_to_account_id = load_in_reply_to_account_id(&db, &quote).await?;
        quotes.push((
            quote.created_at.clone(),
            quote.id.clone(),
            serde_json::to_value(
                build_local_status_response(
                    &db,
                    &config,
                    viewer.as_ref(),
                    &quote,
                    &account,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            )?,
        ));
    }

    for quote in list_remote_status_quotes_by_uri(&db, &target_uri, &cursor, query_limit).await? {
        if !is_public_activitypub_visibility(&quote.visibility) {
            continue;
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &quote.actor_uri).await? else {
            continue;
        };
        quotes.push((
            quote.published_at.clone(),
            quote.id.clone(),
            serde_json::to_value(
                build_remote_status_response(&db, &config, viewer.as_ref(), &quote, &actor).await?,
            )?,
        ));
    }

    quotes.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let first_id = quotes
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = quotes
        .last()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let values = quotes
        .into_iter()
        .take(limit as usize)
        .map(|(_, _, value)| value)
        .collect::<Vec<_>>();
    let mut builder = Response::from_json(&values)?;
    if let Some(link) =
        build_timeline_link_header(&req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}

async fn enqueue_quote_revocation_federation(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    target_status_id: &str,
    target_uri: &str,
    interacting_object_uri: &str,
    authorization_key: &str,
    follower_inboxes: &[String],
    remote_quote_author_actor_uri: Option<&str>,
) -> Result<()> {
    let payload = build_delete_quote_authorization_activity(
        config,
        requester,
        interacting_object_uri,
        target_uri,
        authorization_key,
    )?;

    let mut unique_follower_inboxes = Vec::new();
    let mut seen = HashSet::new();
    for inbox in follower_inboxes {
        let inbox = inbox.trim();
        if !inbox.is_empty() && seen.insert(inbox.to_owned()) {
            unique_follower_inboxes.push(inbox.to_owned());
        }
    }
    if !unique_follower_inboxes.is_empty() {
        enqueue_targeted_outbox_activity(
            db,
            &requester.id,
            target_status_id,
            &payload,
            &unique_follower_inboxes,
        )
        .await?;
    }
    if let Some(actor_uri) = remote_quote_author_actor_uri {
        let _ = queue_remote_actor_activity(db, &requester.id, actor_uri, &payload).await?;
    }

    Ok(())
}

pub(crate) async fn revoke_quote_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let requester = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let Some(target_status_id) = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(quote_status_id) = ctx
        .param("quote_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Response::error("missing quote status id route parameter", 400);
    };

    let Some(target_status) = resolve_status_reference(&db, &config, &target_status_id).await?
    else {
        return Response::error("status not found", 404);
    };
    let (target_status_id, target_uri) = match &target_status {
        crate::ResolvedStatus::Local(status) => {
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                return Response::error("status not found", 404);
            };
            if status.account_id != requester.id
                || !can_view_local_status(&db, &status, Some(&requester), &account).await?
            {
                return Response::error("status not found", 404);
            }
            (status.id.clone(), local_status_target_uri(status))
        }
        crate::ResolvedStatus::Remote(status) => {
            let _ = status;
            return Response::error("status not found", 404);
        }
    };
    let mut quote_revocation_targets = list_follower_delivery_targets(&db, &requester.id).await?;

    match resolve_status_reference(&db, &config, &quote_status_id).await? {
        Some(crate::ResolvedStatus::Local(quote_status)) => {
            if quote_status.quote_of_uri.as_deref() != Some(target_uri.as_str())
                || !status_has_active_quote(&quote_status)
            {
                return Response::error("status not found", 404);
            }
            let Some(quote_author) = find_account_by_id(&db, &quote_status.account_id).await?
            else {
                return Response::error("status not found", 404);
            };

            let current_media = find_media_attachments_by_status_id(&db, &quote_status.id).await?;
            let previous_in_reply_to_account_id =
                load_in_reply_to_account_id(&db, &quote_status).await?;
            let previous_response = build_local_status_response(
                &db,
                &config,
                Some(&requester),
                &quote_status,
                &quote_author,
                previous_in_reply_to_account_id,
                current_media.clone(),
            )
            .await?;
            let mut previous_snapshot =
                serde_json::to_value(previous_response).unwrap_or_else(|_| serde_json::json!({}));
            let revision_at = now_iso_string()?;
            previous_snapshot["created_at"] = serde_json::json!(revision_at.clone());
            let previous_snapshot = normalize_status_history_entry(previous_snapshot);
            let previous_snapshot_json =
                serde_json::to_string(&previous_snapshot).map_err(|error| {
                    worker::Error::RustError(format!(
                        "failed to serialize status snapshot: {error}"
                    ))
                })?;
            insert_status_edit_snapshot(
                &db,
                &quote_status.id,
                &previous_snapshot_json,
                &revision_at,
            )
            .await?;

            let updated_status = clear_local_status_quote(&db, &quote_status, &revision_at).await?;
            enqueue_status_update_activity(&db, &config, &quote_author, &updated_status).await?;
            quote_revocation_targets
                .extend(list_follower_delivery_targets(&db, &quote_author.id).await?);
            enqueue_quote_revocation_federation(
                &db,
                &config,
                &requester,
                &target_status_id,
                &target_uri,
                &local_status_target_uri(&updated_status),
                &updated_status.id,
                &quote_revocation_targets,
                None,
            )
            .await?;

            let media = find_media_attachments_by_status_id(&db, &updated_status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(&db, &updated_status).await?;
            let response = build_local_status_response(
                &db,
                &config,
                Some(&requester),
                &updated_status,
                &quote_author,
                in_reply_to_account_id,
                media,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(crate::ResolvedStatus::Remote(quote_status)) => {
            if quote_status.quote_of_uri.as_deref() != Some(target_uri.as_str())
                || !remote_status_has_active_quote(&quote_status)
            {
                return Response::error("status not found", 404);
            }
            let Some(quote_author) =
                find_remote_actor_by_actor_uri(&db, &quote_status.actor_uri).await?
            else {
                return Response::error("status not found", 404);
            };
            let updated_status =
                update_remote_status_quote_state(&db, &quote_status.id, "revoked").await?;
            enqueue_quote_revocation_federation(
                &db,
                &config,
                &requester,
                &target_status_id,
                &target_uri,
                &quote_status.object_uri,
                &quote_status.id,
                &quote_revocation_targets,
                Some(&quote_author.actor_uri),
            )
            .await?;
            let response = build_remote_status_response(
                &db,
                &config,
                Some(&requester),
                &updated_status,
                &quote_author,
            )
            .await?;
            Response::from_json(&response)
        }
        None => Response::error("status not found", 404),
    }
}

pub(crate) async fn accounts_index_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let mut response = Vec::new();

    for account_id in parse_relationship_query_ids(&req)? {
        match resolve_account_reference(&db, &account_id).await? {
            Some(AccountReference::Local(account)) => {
                let stats = load_account_stats(&db, &account.id).await?;
                response.push(crate::MastodonAccountResponse::from_account_with_stats(
                    &account, &config, &stats,
                ));
            }
            Some(AccountReference::Remote(actor)) => {
                response.push(crate::MastodonAccountResponse::from_remote_actor(&actor));
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
    let db = ctx.d1(&config.database_binding)?;
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
    let agreement = match parse_optional_bool(request.agreement.as_deref()) {
        Ok(value) => value,
        Err(_) => None,
    };
    let mut details = validate_account_registration_request(&AccountRegistrationValidation {
        username: request.username.clone(),
        email: request.email.clone(),
        password_present: request.password.is_some(),
        agreement,
    });
    if let Some(username) = request.username.as_deref()
        && find_account_by_username(&db, username).await?.is_some()
    {
        details
            .entry("username")
            .or_default()
            .push("has already been taken".to_owned());
    }
    if let Some(email) = request.email.as_deref()
        && find_account_by_email(&db, email).await?.is_some()
    {
        details
            .entry("email")
            .or_default()
            .push("has already been taken".to_owned());
    }
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
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;

    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, &target.username);
            delete_follow_by_target(&db, &target.id, &actor_url(&config, &viewer.username)).await?;
            let relationship =
                build_relationship_for_target(&db, &config, &viewer, &target.id, &target_actor_uri)
                    .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            let accepted_follow_activity_id = find_follower_follow_activity_id(
                &db,
                &viewer.id,
                &actor.actor_uri,
                &actor.actor_uri,
            )
            .await?;
            if let Some(request) =
                find_pending_remote_follow_request_by_actor(&db, &viewer.id, &actor.actor_uri)
                    .await?
            {
                delete_remote_follow_request_by_actor(
                    &db,
                    &viewer.id,
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
                        &viewer.id,
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
                    &viewer.id,
                    &actor.actor_uri,
                    &payload,
                )
                .await;
            }
            delete_follower_by_actor(&db, &viewer.id, &actor.actor_uri, &actor.actor_uri).await?;
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

async fn list_local_status_quotes_by_uri(
    db: &worker::D1Database,
    status_uri: &str,
    cursor: &crate::ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<crate::StatusRow>> {
    let bindings = quote_cursor_bindings(status_uri, cursor, limit);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE quote_of_uri = ?1
               AND quote_state = 'accepted'
               AND (
                    ?2 IS NULL
                    OR created_at < ?2
                    OR (created_at = ?2 AND id < ?3)
               )
               AND (
                    ?4 IS NULL
                    OR created_at > ?4
                    OR (created_at = ?4 AND id > ?5)
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?6",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<crate::StatusRow>()
}

async fn list_remote_status_quotes_by_uri(
    db: &worker::D1Database,
    status_uri: &str,
    cursor: &crate::ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<crate::RemoteStatusRow>> {
    let bindings = quote_cursor_bindings(status_uri, cursor, limit);
    let result = db
        .prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
             FROM remote_statuses
             WHERE quote_of_uri = ?1
               AND quote_state = 'accepted'
               AND (
                    ?2 IS NULL
                    OR published_at < ?2
                    OR (published_at = ?2 AND id < ?3)
               )
               AND (
                    ?4 IS NULL
                    OR published_at > ?4
                    OR (published_at = ?4 AND id > ?5)
               )
             ORDER BY published_at DESC, id DESC
             LIMIT ?6",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result.results::<crate::RemoteStatusRow>()
}

fn quote_cursor_bindings<'a>(
    status_uri: &'a str,
    cursor: &'a crate::ResolvedTimelineCursor,
    limit: u32,
) -> [D1Type<'a>; 6] {
    [
        D1Type::Text(status_uri),
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ]
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
    fn sse_event_bytes_match_event_stream_format() {
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "event-1".to_owned(),
            event: "update",
            data: "{\"id\":\"status-1\"}".to_owned(),
        };

        assert_eq!(
            sse_event_bytes(&event),
            b"event: update\ndata: {\"id\":\"status-1\"}\n\n".to_vec()
        );
    }
}
