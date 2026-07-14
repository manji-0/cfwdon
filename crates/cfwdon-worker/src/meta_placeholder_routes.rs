use crate::auth::find_account_by_email;
use crate::crypto_keys::generate_account_key_material;
use crate::timelines::{
    TimelinePaginationQuery, build_timeline_link_header, matches_tag_timeline_filters,
    resolve_timeline_cursor, timeline_fetch_limit, timeline_limit,
};
use crate::{
    AccountReference, LocalApiAuthentication, NotificationsQuery, Request, Response, Result,
    RouteContext, StreamingBatch, StreamingEntry, StreamingEvent, StreamingLoopState,
    StreamingPublicPlan, actor_url, app_bearer_token_from_request, authenticate_local_api_request,
    build_accept_quote_request_activity, build_announcements_document,
    build_app_verify_credentials_document_from_parts,
    build_app_verify_credentials_document_from_row, build_create_quote_authorization_activity,
    build_delete_quote_authorization_activity, build_local_status_response,
    build_local_status_response_with_quote_count_preloads, build_oauth_token_document,
    build_quote_request_object, build_reject_follow_activity, build_reject_quote_request_activity,
    build_relationship_for_target, build_remote_status_response,
    build_remote_status_response_with_timeline_preloads, cache_public_response,
    can_view_local_status, clear_local_status_quote, collect_visible_notifications,
    delete_follow_by_target, delete_follower_by_actor, delete_remote_follow_request_by_actor,
    enqueue_status_update_activity, enqueue_targeted_outbox_activity, extract_hashtags_from_html,
    extract_hashtags_from_text, filter_notification_entries_by_query, find_account_by_id,
    find_account_by_username, find_accounts_by_ids, find_authenticated_local_account,
    find_conversation_for_account, find_conversation_id_by_status_id,
    find_follower_follow_activity_id, find_local_status_by_object_uri,
    find_media_attachments_by_status_id, find_media_attachments_by_status_ids,
    find_oauth_access_token_with_account_by_bearer_token, find_oauth_app_by_bearer_token,
    find_oauth_app_id_by_bearer_token, find_pending_remote_follow_request_by_actor,
    find_remote_actor_by_actor_uri, find_remote_actors_by_actor_uris,
    find_remote_status_attachments_by_status_ids, find_remote_status_by_id, find_status_by_id,
    generate_entity_id, insert_status_edit_snapshot, instance_base_url,
    is_local_status_thread_muted_by, is_muted_actor, is_public_activitypub_visibility,
    issue_oauth_access_token, list_announcement_read_ids, list_followed_tag_names,
    list_follower_delivery_targets, list_local_direct_timeline_statuses,
    list_local_home_timeline_statuses, list_local_public_statuses_by_tag,
    list_local_public_timeline_statuses, list_membership_refs,
    list_membership_variants_for_local_account, list_membership_variants_for_remote_actor,
    list_remote_home_timeline_statuses, list_remote_public_statuses_by_tag,
    list_remote_public_timeline_statuses, list_row_by_id, load_account_stats,
    load_announcement_reaction_state, load_config, load_config_from_env,
    load_in_reply_to_account_id, load_in_reply_to_account_ids, load_latest_filter_updated_at,
    load_remote_status_updated_at, load_status_updated_at, local_quote_revoke_allowed,
    local_status_ap_id, local_status_target_uri, media_object_url, normalize_status_history_entry,
    now_iso_string, oauth_access_token_has_any_scope, oauth_app_has_any_scope, oauth_app_scopes,
    parse_optional_bool, parse_relationship_query_ids, preload_local_status_viewer_state,
    preload_mastodon_poll_responses, preload_remote_mastodon_poll_responses,
    preload_remote_status_edit_updated_at, preload_remote_status_viewer_state,
    preload_status_applications, preload_status_counts, preload_status_quote_counts,
    queue_remote_actor_activity, queue_remote_actor_activity_required, quote_authorization_uri,
    quote_request_uri, remote_account_rest_id, remote_status_has_media, resolve_account_reference,
    resolve_status_reference, send_push_notification, store_account_password,
    store_account_private_key, streaming_batch_from_entries, update_local_status_quote_state,
    update_remote_status_quote_state,
};
use async_stream::try_stream;
use cfwdon_domain::{OwnerQuoteAction, QuoteState};
use futures_util::{FutureExt, StreamExt, pin_mut, select};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use worker::{
    D1Database, Env, Fetch, Headers, Method, RequestInit, ResponseBody, WebSocket, WebSocketPair,
    console_error, console_log, d1::D1Type, ws_events::WebsocketEvent,
};

const STREAMING_POLL_INTERVAL_SECS: u64 = 3;
const STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION: u32 = 90;
const STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION: u32 = 200;

#[derive(Debug, Deserialize)]
struct OembedQuery {
    url: String,
    maxwidth: Option<u32>,
    maxheight: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct QuotesQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

impl QuotesQuery {
    fn pagination(&self) -> TimelinePaginationQuery {
        TimelinePaginationQuery {
            limit: self.limit,
            max_id: self.max_id.clone(),
            since_id: self.since_id.clone(),
            min_id: self.min_id.clone(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct StreamingQuery {
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamingWebSocketClientMessage {
    #[serde(rename = "type")]
    message_type: String,
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
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
    let composing = cfwdon_domain::ComposingRegistration {
        username: validation.username.clone(),
        email: validation.email.clone(),
        password_present: validation.password_present,
        agreement: validation.agreement,
    };
    match composing.validate() {
        Ok(_) => BTreeMap::new(),
        Err(errors) => registration_validation_errors_to_details(errors),
    }
}

fn registration_validation_errors_to_details(
    errors: cfwdon_domain::RegistrationValidationErrors,
) -> BTreeMap<&'static str, Vec<String>> {
    let mut details = BTreeMap::new();
    if let Some(issue) = errors.username {
        details.insert(
            "username",
            vec![
                match issue {
                    cfwdon_domain::RegistrationFieldIssue::Blank => "can't be blank",
                    cfwdon_domain::RegistrationFieldIssue::InvalidFormat => {
                        "must contain only letters, numbers and underscores"
                    }
                    cfwdon_domain::RegistrationFieldIssue::MustBeAccepted => "must be accepted",
                }
                .to_owned(),
            ],
        );
    }
    if let Some(issue) = errors.email {
        details.insert(
            "email",
            vec![
                match issue {
                    cfwdon_domain::RegistrationFieldIssue::Blank => "can't be blank",
                    cfwdon_domain::RegistrationFieldIssue::InvalidFormat => "is invalid",
                    cfwdon_domain::RegistrationFieldIssue::MustBeAccepted => "must be accepted",
                }
                .to_owned(),
            ],
        );
    }
    if let Some(issue) = errors.password {
        details.insert(
            "password",
            vec![
                match issue {
                    cfwdon_domain::RegistrationFieldIssue::Blank => "can't be blank",
                    cfwdon_domain::RegistrationFieldIssue::InvalidFormat => "is invalid",
                    cfwdon_domain::RegistrationFieldIssue::MustBeAccepted => "must be accepted",
                }
                .to_owned(),
            ],
        );
    }
    if let Some(issue) = errors.agreement {
        details.insert(
            "agreement",
            vec![
                match issue {
                    cfwdon_domain::RegistrationFieldIssue::Blank => "can't be blank",
                    cfwdon_domain::RegistrationFieldIssue::InvalidFormat => "is invalid",
                    cfwdon_domain::RegistrationFieldIssue::MustBeAccepted => "must be accepted",
                }
                .to_owned(),
            ],
        );
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

fn normalize_streaming_path_channel(value: &str) -> Option<String> {
    let path = value.trim().trim_matches('/');
    if path.is_empty() {
        return None;
    }
    normalize_streaming_channel(Some(&path.replace('/', ":")))
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
    let stream = match extra_path.map(str::trim).filter(|value| !value.is_empty()) {
        Some(_)
            if stream
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some() =>
        {
            return Err(StreamingChannelValidationError::UnknownChannelRequested);
        }
        Some(path) => normalize_streaming_path_channel(path),
        None => normalize_streaming_channel(stream),
    };
    let Some(stream) = stream else {
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
        username = account.username(),
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
        "joined_at": account.created_at(),
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
        LocalApiAuthentication::Auth0(account) => account,
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
        Response::from_json(&serde_json::json!({
            "type": "rich",
            "version": "1.0",
            "title": format!("New status by {}", account.username()),
            "author_name": author_name,
            "author_url": actor_url(&config, account.username()),
            "provider_name": config.instance_domain,
            "provider_url": format!("{}/", oauth_base_url(&config)),
            "cache_age": 86400,
            "html": build_oembed_html(&account, &status_url, &status.content_html),
            "width": query.maxwidth.unwrap_or(400),
            "height": query.maxheight,
        }))?,
        crate::CACHE_TTL_OEMBED,
    )
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
    let db = ctx.d1(&config.database_binding)?;
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
    let db = ctx.d1(&config.database_binding)?;
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
    let db = ctx.d1(&config.database_binding)?;
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
    let db = ctx.d1(&config.database_binding)?;
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
    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_authenticated_local_account(&req, &db, &config).await? else {
        return invalid_access_token_response();
    };
    let confirmed = find_pending_email_confirmation(&db, account.id())
        .await?
        .is_none()
        && !account.access_email().trim().is_empty();
    Response::from_json(&confirmed)
}

struct StreamingWebSocketSubscription {
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
    state: StreamingLoopState,
}

impl StreamingWebSocketSubscription {
    fn new(stream_name: String, tag: Option<String>, list: Option<String>) -> Self {
        Self {
            stream_name,
            tag,
            list,
            state: StreamingLoopState::new(),
        }
    }
}

fn sse_comment_bytes(value: &str) -> Vec<u8> {
    format!(": {value}\n\n").into_bytes()
}

fn sse_event_bytes(event: &StreamingEvent) -> Vec<u8> {
    format!("event: {}\ndata: {}\n\n", event.event, event.data).into_bytes()
}

fn streaming_websocket_subscription_key(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        stream_name,
        tag.unwrap_or_default(),
        list.unwrap_or_default()
    )
}

fn streaming_websocket_stream_labels(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) -> Vec<String> {
    let mut labels = vec![stream_name.to_owned()];
    if stream_name.starts_with("hashtag")
        && let Some(tag) = tag
    {
        labels.push(tag.to_owned());
    }
    if stream_name == "list"
        && let Some(list) = list
    {
        labels.push(list.to_owned());
    }
    labels
}

fn streaming_websocket_event_message(
    subscription: &StreamingWebSocketSubscription,
    event: &StreamingEvent,
) -> Result<String> {
    let mut payload = serde_json::json!({
        "stream": streaming_websocket_stream_labels(
            &subscription.stream_name,
            subscription.tag.as_deref(),
            subscription.list.as_deref(),
        ),
        "event": event.event,
    });
    if event.event != "filters_changed" {
        payload["payload"] = serde_json::Value::String(event.data.clone());
    }
    serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize websocket stream event: {error}"
        ))
    })
}

fn streaming_websocket_error_message(message: &str, status: u16) -> String {
    serde_json::json!({
        "error": message,
        "status": status,
    })
    .to_string()
}

fn streaming_poll_budget_exhausted(poll_rounds: u32, subscription_polls: u32) -> bool {
    poll_rounds >= STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION
        || subscription_polls >= STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION
}

fn streaming_error_is_subrequest_limit(error: &worker::Error) -> bool {
    error
        .to_string()
        .contains("Too many API requests by single Worker invocation")
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

fn streaming_filter_update_changed(previous: Option<&str>, current: &str) -> bool {
    previous.map(|value| value != current).unwrap_or(false)
}

struct AnnouncementStreamEntry {
    id: String,
    payload: String,
    created_at: String,
}

struct CurrentAnnouncementStreamState {
    entries: Vec<AnnouncementStreamEntry>,
    reactions: HashMap<(String, String), (u64, bool)>,
}

fn announcement_stream_entries(
    announcements: Vec<serde_json::Value>,
) -> Result<Vec<AnnouncementStreamEntry>> {
    let mut entries = Vec::new();
    for announcement in announcements {
        if let Some(entry) = announcement_stream_entry(&announcement)? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn announcement_stream_entry(
    announcement: &serde_json::Value,
) -> Result<Option<AnnouncementStreamEntry>> {
    let Some(id) = announcement
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
    else {
        return Ok(None);
    };
    let payload = serde_json::to_string(announcement).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize announcement stream payload: {error}"
        ))
    })?;
    let created_at = announcement
        .get("published_at")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            announcement
                .get("updated_at")
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or_default()
        .to_owned();

    Ok(Some(AnnouncementStreamEntry {
        id,
        payload,
        created_at,
    }))
}

async fn streaming_notification_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    since_id: Option<&str>,
    min_created_at: Option<&str>,
) -> Result<StreamingBatch> {
    let query = NotificationsQuery {
        since_id: since_id.map(str::to_owned),
        min_created_at: min_created_at.map(str::to_owned),
        limit: Some(40),
        ..NotificationsQuery::default()
    };
    let entries = collect_visible_notifications(db, config, viewer, &query, 160).await?;
    let filtered = filter_notification_entries_by_query(entries, &query);
    let last_id = filtered.first().map(|entry| entry.id.clone());
    let last_created_at = filtered.first().map(|entry| entry.created_at.clone());
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
        last_created_at,
    })
}

async fn append_streaming_local_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: crate::StatusRow,
    only_media: bool,
    mute_local_actor: bool,
    tag_filter: Option<&str>,
    include_reply_context: bool,
    payload_context: &str,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if let Some(tag) = tag_filter {
        let status_tags = extract_hashtags_from_text(&status.text);
        if !matches_tag_timeline_filters(&status_tags, tag, &crate::TagTimelineQuery::default()) {
            return Ok(());
        }
    }
    let Some(account) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(());
    };
    if mute_local_actor
        && let Some(viewer) = viewer
        && is_muted_actor(db, viewer.id(), &actor_url(config, account.username())).await?
    {
        return Ok(());
    }
    if let Some(viewer) = viewer
        && is_local_status_thread_muted_by(db, viewer.id(), &status).await?
    {
        return Ok(());
    }
    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    if only_media && media.is_empty() {
        return Ok(());
    }
    let in_reply_to_account_id = if include_reply_context {
        load_in_reply_to_account_id(db, &status).await?
    } else {
        None
    };
    entries.push(StreamingEntry::new(
        status.created_at.clone(),
        status.id.clone(),
        serde_json::to_string(
            &build_local_status_response(
                db,
                config,
                viewer,
                &status,
                &account,
                in_reply_to_account_id,
                media,
            )
            .await?,
        )
        .map_err(|error| {
            worker::Error::RustError(format!(
                "failed to serialize {payload_context} stream payload: {error}"
            ))
        })?,
    ));
    tracked_status_ids.push(status.id);
    Ok(())
}

async fn append_streaming_remote_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: crate::RemoteStatusRow,
    actor: crate::RemoteActorRow,
    only_media: bool,
    tag_filter: Option<&str>,
    payload_context: &str,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if let Some(tag) = tag_filter {
        let status_tags = extract_hashtags_from_html(&status.content_html);
        if !matches_tag_timeline_filters(&status_tags, tag, &crate::TagTimelineQuery::default()) {
            return Ok(());
        }
    }
    if only_media && !remote_status_has_media(db, &status.id).await? {
        return Ok(());
    }
    if let Some(viewer) = viewer
        && is_muted_actor(db, viewer.id(), &actor.actor_uri).await?
    {
        return Ok(());
    }
    entries.push(StreamingEntry::new(
        status.published_at.clone(),
        status.id.clone(),
        serde_json::to_string(
            &build_remote_status_response(db, config, viewer, &status, &actor).await?,
        )
        .map_err(|error| {
            worker::Error::RustError(format!(
                "failed to serialize {payload_context} stream payload: {error}"
            ))
        })?,
    ));
    tracked_status_ids.push(status.id);
    Ok(())
}

async fn streaming_public_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    stream: &str,
    tag: Option<&str>,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let plan = StreamingPublicPlan::from_stream(stream);
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

    if plan.hashtag_stream {
        let Some(tag) = tag else {
            return Ok(StreamingBatch::empty());
        };
        append_streaming_hashtag_status_entries(
            db,
            config,
            viewer,
            plan,
            tag,
            &cursor,
            query_limit,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    } else {
        append_streaming_public_status_entries(
            db,
            config,
            viewer,
            plan,
            &cursor,
            query_limit,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    }

    Ok(streaming_batch_from_entries(
        entries,
        tracked_status_ids,
        "conversation",
    ))
}

async fn append_streaming_hashtag_status_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    plan: StreamingPublicPlan,
    tag: &str,
    cursor: &crate::ResolvedTimelineCursor,
    query_limit: u32,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if plan.include_local {
        for status in list_local_public_statuses_by_tag(db, tag, cursor, query_limit).await? {
            append_streaming_local_status_entry(
                db,
                config,
                viewer,
                status,
                plan.only_media,
                false,
                Some(tag),
                true,
                "hashtag",
                entries,
                tracked_status_ids,
            )
            .await?;
        }
    }
    if plan.include_remote {
        for (status, actor) in
            list_remote_public_statuses_by_tag(db, tag, cursor, query_limit).await?
        {
            append_streaming_remote_status_entry(
                db,
                config,
                viewer,
                status,
                actor,
                plan.only_media,
                Some(tag),
                "hashtag",
                entries,
                tracked_status_ids,
            )
            .await?;
        }
    }
    Ok(())
}

async fn append_streaming_public_status_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    plan: StreamingPublicPlan,
    cursor: &crate::ResolvedTimelineCursor,
    query_limit: u32,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if plan.include_local {
        for status in list_local_public_timeline_statuses(db, cursor, query_limit).await? {
            append_streaming_local_status_entry(
                db,
                config,
                viewer,
                status,
                plan.only_media,
                false,
                None,
                false,
                "public",
                entries,
                tracked_status_ids,
            )
            .await?;
        }
    }
    if plan.include_remote {
        for (status, actor) in list_remote_public_timeline_statuses(db, cursor, query_limit).await?
        {
            append_streaming_remote_status_entry(
                db,
                config,
                viewer,
                status,
                actor,
                plan.only_media,
                None,
                "public",
                entries,
                tracked_status_ids,
            )
            .await?;
        }
    }
    Ok(())
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

    for status in list_local_home_timeline_statuses(db, viewer.id(), &cursor, query_limit).await? {
        append_streaming_home_local_status_entry(
            db,
            config,
            viewer,
            status,
            &mut seen_status_ids,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    }

    for (status, actor) in
        list_remote_home_timeline_statuses(db, viewer.id(), &cursor, query_limit).await?
    {
        append_streaming_home_remote_status_entry(
            db,
            config,
            viewer,
            status,
            actor,
            &mut seen_status_ids,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    }

    for tag in list_followed_tag_names(db, viewer.id()).await? {
        for status in list_local_public_statuses_by_tag(db, &tag, &cursor, query_limit).await? {
            append_streaming_home_local_status_entry(
                db,
                config,
                viewer,
                status,
                &mut seen_status_ids,
                &mut entries,
                &mut tracked_status_ids,
            )
            .await?;
        }

        for (status, actor) in
            list_remote_public_statuses_by_tag(db, &tag, &cursor, query_limit).await?
        {
            append_streaming_home_remote_status_entry(
                db,
                config,
                viewer,
                status,
                actor,
                &mut seen_status_ids,
                &mut entries,
                &mut tracked_status_ids,
            )
            .await?;
        }
    }

    Ok(streaming_batch_from_entries(
        entries,
        tracked_status_ids,
        "update",
    ))
}

async fn append_streaming_home_local_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    status: crate::StatusRow,
    seen_status_ids: &mut HashSet<String>,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if !seen_status_ids.insert(status.id.clone()) {
        return Ok(());
    }
    append_streaming_local_status_entry(
        db,
        config,
        Some(viewer),
        status,
        false,
        true,
        None,
        true,
        "home",
        entries,
        tracked_status_ids,
    )
    .await
}

async fn append_streaming_home_remote_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    status: crate::RemoteStatusRow,
    actor: crate::RemoteActorRow,
    seen_status_ids: &mut HashSet<String>,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if !seen_status_ids.insert(status.id.clone()) {
        return Ok(());
    }
    append_streaming_remote_status_entry(
        db,
        config,
        Some(viewer),
        status,
        actor,
        false,
        None,
        "home",
        entries,
        tracked_status_ids,
    )
    .await
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

    for status in list_local_direct_timeline_statuses(db, viewer.id(), &cursor, query_limit).await?
    {
        let Some(conversation_id) = find_conversation_id_by_status_id(db, &status.id).await? else {
            continue;
        };
        if !seen_conversation_ids.insert(conversation_id.clone()) {
            continue;
        }
        let Some(conversation) =
            find_conversation_for_account(db, viewer.id(), &conversation_id).await?
        else {
            continue;
        };
        entries.push(StreamingEntry::new(
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

    Ok(streaming_batch_from_entries(
        entries,
        tracked_conversation_ids,
        "update",
    ))
}

async fn streaming_list_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    list_id: &str,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let Some(context) = streaming_list_batch_context(db, viewer, list_id, since_id).await? else {
        return Ok(StreamingBatch::empty());
    };
    let StreamingListBatchContext {
        cursor,
        query_limit,
        membership_refs,
        replies_policy,
    } = context;
    let mut entries = Vec::new();
    let mut tracked_status_ids = Vec::new();
    let policy = ListStreamStatusPolicy::new(&membership_refs, &replies_policy);

    for status in list_local_public_timeline_statuses(db, &cursor, query_limit).await? {
        append_streaming_list_local_status_entry(
            db,
            config,
            viewer,
            &policy,
            status,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    }

    for (status, actor) in list_remote_public_timeline_statuses(db, &cursor, query_limit).await? {
        append_streaming_list_remote_status_entry(
            db,
            config,
            viewer,
            &policy,
            status,
            actor,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    }

    Ok(streaming_batch_from_entries(
        entries,
        tracked_status_ids,
        "update",
    ))
}

struct StreamingListBatchContext {
    cursor: crate::ResolvedTimelineCursor,
    query_limit: u32,
    membership_refs: HashSet<String>,
    replies_policy: String,
}

async fn streaming_list_batch_context(
    db: &D1Database,
    viewer: &crate::LocalAccount,
    list_id: &str,
    since_id: Option<&str>,
) -> Result<Option<StreamingListBatchContext>> {
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
    let Some(list) = list_row_by_id(db, viewer.id(), list_id).await? else {
        return Ok(None);
    };
    let membership_refs = list_membership_refs(db, list_id)
        .await?
        .into_iter()
        .map(|row| row.target_account_ref)
        .collect::<HashSet<_>>();
    Ok(Some(StreamingListBatchContext {
        cursor,
        query_limit,
        membership_refs,
        replies_policy: list.replies_policy,
    }))
}

async fn append_streaming_list_local_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    policy: &ListStreamStatusPolicy<'_>,
    status: crate::StatusRow,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    let Some(author) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(());
    };
    if !policy.matches(
        list_membership_variants_for_local_account(&author, config),
        status.in_reply_to_id.as_deref(),
    ) {
        return Ok(());
    }
    if is_local_status_thread_muted_by(db, viewer.id(), &status).await? {
        return Ok(());
    }
    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    entries.push(StreamingEntry::new(
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
            worker::Error::RustError(format!("failed to serialize list stream payload: {error}"))
        })?,
    ));
    tracked_status_ids.push(status.id.clone());
    Ok(())
}

async fn append_streaming_list_remote_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    policy: &ListStreamStatusPolicy<'_>,
    status: crate::RemoteStatusRow,
    actor: crate::RemoteActorRow,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if !policy.matches(
        list_membership_variants_for_remote_actor(&actor),
        status.in_reply_to_uri.as_deref(),
    ) {
        return Ok(());
    }
    if is_muted_actor(db, viewer.id(), &actor.actor_uri).await? {
        return Ok(());
    }
    entries.push(StreamingEntry::new(
        status.published_at.clone(),
        status.id.clone(),
        serde_json::to_string(
            &build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
        )
        .map_err(|error| {
            worker::Error::RustError(format!("failed to serialize list stream payload: {error}"))
        })?,
    ));
    tracked_status_ids.push(status.id.clone());
    Ok(())
}

struct ListStreamStatusPolicy<'a> {
    membership_refs: &'a HashSet<String>,
    replies_policy: &'a str,
}

impl<'a> ListStreamStatusPolicy<'a> {
    fn new(membership_refs: &'a HashSet<String>, replies_policy: &'a str) -> Self {
        Self {
            membership_refs,
            replies_policy,
        }
    }

    fn matches(
        &self,
        candidates: impl IntoIterator<Item = String>,
        reply_reference: Option<&str>,
    ) -> bool {
        list_stream_membership_refs_include_any(self.membership_refs, candidates)
            && !list_stream_excludes_reply(self.replies_policy, reply_reference)
    }
}

fn list_stream_membership_refs_include_any(
    membership_refs: &HashSet<String>,
    candidates: impl IntoIterator<Item = String>,
) -> bool {
    candidates
        .into_iter()
        .any(|candidate| membership_refs.contains(&candidate))
}

fn list_stream_excludes_reply(replies_policy: &str, reply_reference: Option<&str>) -> bool {
    replies_policy == "none" && reply_reference.is_some()
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
        append_streaming_status_delta_event(
            db,
            config,
            viewer,
            status_id,
            deleted_status_ids,
            updated_status_ids,
            &mut events,
        )
        .await?;
    }

    Ok(events)
}

async fn append_streaming_status_delta_event(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status_id: &str,
    deleted_status_ids: &mut HashSet<String>,
    updated_status_ids: &mut HashSet<String>,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    if streaming_status_delta_already_recorded(status_id, deleted_status_ids, updated_status_ids) {
        return Ok(());
    }

    if let Some(status) = find_status_by_id(db, status_id).await? {
        if let Some(event) =
            streaming_local_status_update_event(db, config, viewer, &status).await?
        {
            updated_status_ids.insert(status.id.clone());
            events.push(event);
        }
        return Ok(());
    }

    if let Some(status) = find_remote_status_by_id(db, status_id).await? {
        if let Some(event) =
            streaming_remote_status_update_event(db, config, viewer, &status).await?
        {
            updated_status_ids.insert(status.id.clone());
            events.push(event);
        }
        return Ok(());
    }

    deleted_status_ids.insert(status_id.to_owned());
    events.push(streaming_status_delete_event(status_id, now_iso_string()?));
    Ok(())
}

fn streaming_status_delta_already_recorded(
    status_id: &str,
    deleted_status_ids: &HashSet<String>,
    updated_status_ids: &HashSet<String>,
) -> bool {
    deleted_status_ids.contains(status_id) || updated_status_ids.contains(status_id)
}

async fn streaming_local_status_update_event(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: &crate::StatusRow,
) -> Result<Option<StreamingEvent>> {
    let Some(updated_at) = load_status_updated_at(db, &status.id).await? else {
        return Ok(None);
    };
    if updated_at == status.created_at {
        return Ok(None);
    }
    let Some(account) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(None);
    };
    if let Some(viewer) = viewer
        && is_local_status_thread_muted_by(db, viewer.id(), status).await?
    {
        return Ok(None);
    }
    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    let payload = build_local_status_response(
        db,
        config,
        viewer,
        status,
        &account,
        load_in_reply_to_account_id(db, status).await?,
        media,
    )
    .await?;
    let data = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize streaming local status update payload: {error}"
        ))
    })?;

    Ok(Some(StreamingEvent {
        created_at: updated_at,
        id: status.id.clone(),
        event: "status.update",
        data,
    }))
}

async fn streaming_remote_status_update_event(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: &crate::RemoteStatusRow,
) -> Result<Option<StreamingEvent>> {
    let Some(updated_at) = load_remote_status_updated_at(db, &status.id).await? else {
        return Ok(None);
    };
    if updated_at == status.published_at {
        return Ok(None);
    }
    if let Some(viewer) = viewer
        && is_muted_actor(db, viewer.id(), &status.actor_uri).await?
    {
        return Ok(None);
    }
    let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
        return Ok(None);
    };
    let payload = build_remote_status_response(db, config, viewer, status, &actor).await?;
    let data = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize streaming remote status update payload: {error}"
        ))
    })?;

    Ok(Some(StreamingEvent {
        created_at: updated_at,
        id: status.id.clone(),
        event: "status.update",
        data,
    }))
}

fn streaming_status_delete_event(status_id: &str, created_at: String) -> StreamingEvent {
    StreamingEvent {
        created_at,
        id: status_id.to_owned(),
        event: "delete",
        data: status_id.to_owned(),
    }
}

fn apply_streaming_batch_to_state(
    stream_name: &str,
    batch: StreamingBatch,
    is_initial_poll: bool,
    state: &mut StreamingLoopState,
) -> Vec<StreamingEvent> {
    if let Some(next_since_id) = batch.last_id {
        state.since_id = Some(next_since_id);
    }
    if stream_name == "user:notification"
        && let Some(next_min_created_at) = batch.last_created_at
    {
        state.notification_min_created_at = Some(next_min_created_at);
    }
    for status_id in batch.tracked_status_ids {
        if state.tracked_status_id_set.insert(status_id.clone()) {
            state.tracked_status_ids.push(status_id);
        }
    }
    while state.tracked_status_ids.len() > 200 {
        let removed = state.tracked_status_ids.remove(0);
        state.tracked_status_id_set.remove(&removed);
    }

    if is_initial_poll {
        for event in &batch.events {
            state.emitted_event_ids.insert(streaming_event_key(event));
        }
        Vec::new()
    } else {
        batch.events
    }
}

fn streaming_event_key(event: &StreamingEvent) -> String {
    format!("{}:{}", event.event, event.id)
}

async fn append_user_stream_state_events(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    state: &mut StreamingLoopState,
    is_initial_poll: bool,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    append_user_filter_state_events(db, viewer, state, is_initial_poll, events).await?;
    append_user_announcement_state_events(config, db, viewer, state, is_initial_poll, events).await
}

async fn append_user_filter_state_events(
    db: &D1Database,
    viewer: &crate::LocalAccount,
    state: &mut StreamingLoopState,
    is_initial_poll: bool,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    let current_filter_updated_at = load_latest_filter_updated_at(db, viewer.id()).await?;
    if let Some(current_filter_updated_at) = current_filter_updated_at {
        let changed = streaming_filter_update_changed(
            state.last_filter_updated_at.as_deref(),
            &current_filter_updated_at,
        );
        if !is_initial_poll && changed {
            events.push(StreamingEvent {
                created_at: current_filter_updated_at.clone(),
                id: current_filter_updated_at.clone(),
                event: "filters_changed",
                data: "undefined".to_owned(),
            });
        }
        state.last_filter_updated_at = Some(current_filter_updated_at);
    }

    Ok(())
}

async fn append_user_announcement_state_events(
    config: &cfwdon_core::AppConfig,
    db: &D1Database,
    viewer: &crate::LocalAccount,
    state: &mut StreamingLoopState,
    is_initial_poll: bool,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    let current_state = load_current_announcement_stream_state(config, db, viewer).await?;
    let mut current_announcements = HashMap::<String, String>::new();

    for entry in current_state.entries {
        append_current_announcement_stream_entry_events(
            &entry,
            is_initial_poll,
            &state.last_announcements,
            &state.last_announcement_reactions,
            &current_state.reactions,
            events,
        );
        current_announcements.insert(entry.id, entry.payload);
    }

    if !is_initial_poll {
        for removed_id in
            removed_announcement_ids(&state.last_announcements, &current_announcements)
        {
            events.push(announcement_delete_event(removed_id, now_iso_string()?));
        }
    }
    state.last_announcement_reactions = current_state.reactions;
    state.last_announcements = current_announcements;
    Ok(())
}

async fn load_current_announcement_stream_state(
    config: &cfwdon_core::AppConfig,
    db: &D1Database,
    viewer: &crate::LocalAccount,
) -> Result<CurrentAnnouncementStreamState> {
    let read_ids = list_announcement_read_ids(db, viewer.id()).await?;
    let reactions = load_announcement_reaction_state(db, viewer.id()).await?;
    let announcements = build_announcements_document(config, &read_ids, &reactions);

    Ok(CurrentAnnouncementStreamState {
        entries: announcement_stream_entries(announcements)?,
        reactions,
    })
}

fn append_current_announcement_stream_entry_events(
    entry: &AnnouncementStreamEntry,
    is_initial_poll: bool,
    previous_announcements: &HashMap<String, String>,
    previous_reactions_state: &HashMap<(String, String), (u64, bool)>,
    current_reactions_state: &HashMap<(String, String), (u64, bool)>,
    events: &mut Vec<StreamingEvent>,
) {
    let current_reactions =
        announcement_reaction_entries_for_id(current_reactions_state, &entry.id);
    let previous_reactions =
        announcement_reaction_entries_for_id(previous_reactions_state, &entry.id);
    match announcement_stream_entry_action(
        is_initial_poll,
        previous_announcements.get(&entry.id).map(String::as_str),
        &entry.payload,
        &current_reactions,
        &previous_reactions,
    ) {
        AnnouncementStreamEntryAction::Reaction => {
            append_announcement_reaction_events(
                entry,
                &current_reactions,
                previous_reactions_state,
                events,
            );
        }
        AnnouncementStreamEntryAction::Announcement => {
            events.push(announcement_stream_event(entry));
        }
        AnnouncementStreamEntryAction::None => {}
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AnnouncementStreamEntryAction {
    None,
    Reaction,
    Announcement,
}

fn announcement_stream_entry_action(
    is_initial_poll: bool,
    previous_payload: Option<&str>,
    current_payload: &str,
    current_reactions: &BTreeMap<String, (u64, bool)>,
    previous_reactions: &BTreeMap<String, (u64, bool)>,
) -> AnnouncementStreamEntryAction {
    if is_initial_poll {
        return AnnouncementStreamEntryAction::None;
    }
    if current_reactions != previous_reactions {
        return AnnouncementStreamEntryAction::Reaction;
    }
    if previous_payload != Some(current_payload) {
        return AnnouncementStreamEntryAction::Announcement;
    }
    AnnouncementStreamEntryAction::None
}

fn announcement_stream_event(entry: &AnnouncementStreamEntry) -> StreamingEvent {
    StreamingEvent {
        created_at: entry.created_at.clone(),
        id: entry.id.clone(),
        event: "announcement",
        data: entry.payload.clone(),
    }
}

fn removed_announcement_ids(
    previous_announcements: &HashMap<String, String>,
    current_announcements: &HashMap<String, String>,
) -> Vec<String> {
    previous_announcements
        .keys()
        .filter(|id| !current_announcements.contains_key(*id))
        .cloned()
        .collect()
}

fn announcement_delete_event(removed_id: String, created_at: String) -> StreamingEvent {
    StreamingEvent {
        created_at,
        id: removed_id.clone(),
        event: "announcement.delete",
        data: removed_id,
    }
}

fn append_announcement_reaction_events(
    entry: &AnnouncementStreamEntry,
    current_reactions: &BTreeMap<String, (u64, bool)>,
    last_announcement_reactions: &HashMap<(String, String), (u64, bool)>,
    events: &mut Vec<StreamingEvent>,
) {
    for (name, (count, me)) in current_reactions {
        let previous = last_announcement_reactions
            .get(&(entry.id.clone(), name.clone()))
            .copied();
        if previous != Some((*count, *me)) {
            events.push(StreamingEvent {
                created_at: entry.created_at.clone(),
                id: format!("{}:{name}", entry.id),
                event: "announcement.reaction",
                data: serde_json::json!({
                    "name": name,
                    "count": count,
                    "announcement_id": entry.id,
                })
                .to_string(),
            });
        }
    }
}

async fn poll_streaming_events(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
    viewer: Option<&crate::LocalAccount>,
    state: &mut StreamingLoopState,
) -> Result<Vec<StreamingEvent>> {
    let is_initial_poll = !state.initialized;
    let batch =
        streaming_batch_for_stream(db, config, stream_name, tag, list, viewer, state).await?;
    let mut events = apply_streaming_batch_to_state(stream_name, batch, is_initial_poll, state);
    append_streaming_poll_side_effect_events(
        db,
        config,
        stream_name,
        viewer,
        state,
        is_initial_poll,
        &mut events,
    )
    .await?;
    state.initialized = true;
    retain_new_streaming_events(state, &mut events);
    Ok(events)
}

async fn streaming_batch_for_stream(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
    viewer: Option<&crate::LocalAccount>,
    state: &StreamingLoopState,
) -> Result<StreamingBatch> {
    match stream_name {
        "user" => {
            let viewer = required_streaming_viewer(viewer, "user")?;
            streaming_home_batch(db, config, viewer, state.since_id.as_deref()).await
        }
        "user:notification" => {
            let viewer = required_streaming_viewer(viewer, "notification")?;
            streaming_notification_batch(
                db,
                config,
                viewer,
                state.since_id.as_deref(),
                state.notification_min_created_at.as_deref(),
            )
            .await
        }
        "list" => {
            let viewer = required_streaming_viewer(viewer, "list")?;
            let list_id = required_streaming_list_id(list)?;
            streaming_list_batch(db, config, viewer, list_id, state.since_id.as_deref()).await
        }
        "direct" => {
            let viewer = required_streaming_viewer(viewer, "direct")?;
            streaming_direct_batch(db, config, viewer, state.since_id.as_deref()).await
        }
        _ => {
            streaming_public_batch(
                db,
                config,
                viewer,
                stream_name,
                tag,
                state.since_id.as_deref(),
            )
            .await
        }
    }
}

fn required_streaming_viewer<'a>(
    viewer: Option<&'a crate::LocalAccount>,
    stream_label: &str,
) -> Result<&'a crate::LocalAccount> {
    viewer.ok_or_else(|| {
        worker::Error::RustError(format!(
            "missing authenticated viewer for {stream_label} stream"
        ))
    })
}

fn required_streaming_list_id(list: Option<&str>) -> Result<&str> {
    list.filter(|value| !value.trim().is_empty())
        .ok_or_else(|| worker::Error::RustError("missing list id for list stream".to_owned()))
}

async fn append_streaming_poll_side_effect_events(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    stream_name: &str,
    viewer: Option<&crate::LocalAccount>,
    state: &mut StreamingLoopState,
    is_initial_poll: bool,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    if !is_initial_poll && stream_name != "user:notification" {
        let delta_events = streaming_status_delta_events(
            db,
            config,
            viewer,
            &state.tracked_status_ids,
            &mut state.deleted_status_ids,
            &mut state.updated_status_ids,
        )
        .await?;
        events.extend(delta_events);
    }
    if stream_name == "user" {
        let viewer = required_streaming_viewer(viewer, "user")?;
        append_user_stream_state_events(db, config, viewer, state, is_initial_poll, events).await?;
    }

    Ok(())
}

fn retain_new_streaming_events(state: &mut StreamingLoopState, events: &mut Vec<StreamingEvent>) {
    events.retain(|event| state.emitted_event_ids.insert(streaming_event_key(event)));
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
        let mut state = StreamingLoopState::new();
        let mut poll_rounds = 0_u32;
        loop {
            poll_rounds = poll_rounds.saturating_add(1);
            let events = match poll_streaming_events(
                    &db,
                    &config,
                    &stream_name,
                    tag.as_deref(),
                    list.as_deref(),
                    viewer.as_ref(),
                    &mut state,
                )
                .await {
                    Ok(events) => events,
                    Err(error) => {
                        console_error!(
                            "streaming poll failed stream={} tag={} list={} error={}",
                            stream_name,
                            tag.as_deref().unwrap_or(""),
                            list.as_deref().unwrap_or(""),
                            error
                        );
                        yield sse_comment_bytes("error=streaming_poll_failed");
                        if streaming_error_is_subrequest_limit(&error)
                            || streaming_poll_budget_exhausted(poll_rounds, poll_rounds)
                        {
                            yield sse_comment_bytes("stream=recycle");
                            break;
                        }
                        worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).await;
                        continue;
                    }
                };
            if events.is_empty() {
                yield sse_comment_bytes("thump");
            } else {
                for event in events {
                    yield sse_event_bytes(&event);
                }
            }
            if streaming_poll_budget_exhausted(poll_rounds, poll_rounds) {
                yield sse_comment_bytes("stream=recycle");
                break;
            }
            worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).await;
        }
    }
}

fn add_streaming_websocket_subscription(
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
) {
    let key = streaming_websocket_subscription_key(&stream_name, tag.as_deref(), list.as_deref());
    subscriptions
        .entry(key)
        .or_insert_with(|| StreamingWebSocketSubscription::new(stream_name, tag, list));
}

fn remove_streaming_websocket_subscription(
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) {
    let key = streaming_websocket_subscription_key(stream_name, tag, list);
    subscriptions.remove(&key);
}

fn handle_streaming_websocket_client_message(
    websocket: &WebSocket,
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    text: &str,
    viewer: Option<&crate::LocalAccount>,
) -> bool {
    let message = match serde_json::from_str::<StreamingWebSocketClientMessage>(text) {
        Ok(message) => message,
        Err(error) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                &format!("Malformed streaming message: {error}"),
                400,
            ));
            return true;
        }
    };
    if !matches!(message.message_type.as_str(), "subscribe" | "unsubscribe") {
        let _ = websocket.send_with_str(streaming_websocket_error_message(
            "Unknown streaming message type",
            400,
        ));
        return true;
    }
    let stream_name = match validate_streaming_channel_request(
        message.stream.as_deref(),
        message.tag.as_deref(),
        message.list.as_deref(),
        None,
    ) {
        Ok(stream_name) => stream_name,
        Err(StreamingChannelValidationError::UnknownChannelRequested) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Unknown stream type",
                400,
            ));
            return true;
        }
        Err(StreamingChannelValidationError::MissingTag) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Missing tag parameter",
                400,
            ));
            return true;
        }
        Err(StreamingChannelValidationError::MissingList) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Missing list parameter",
                400,
            ));
            return true;
        }
    };
    if streaming_channel_requires_auth(&stream_name) && viewer.is_none() {
        let _ = websocket.send_with_str(streaming_websocket_error_message(
            "The access token is invalid",
            401,
        ));
        return true;
    }
    if message.message_type == "subscribe" {
        add_streaming_websocket_subscription(subscriptions, stream_name, message.tag, message.list);
    } else {
        remove_streaming_websocket_subscription(
            subscriptions,
            &stream_name,
            message.tag.as_deref(),
            message.list.as_deref(),
        );
    }
    true
}

async fn poll_streaming_websocket_subscriptions(
    websocket: &WebSocket,
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
) -> bool {
    for subscription in subscriptions.values_mut() {
        let events = match poll_streaming_events(
            db,
            config,
            &subscription.stream_name,
            subscription.tag.as_deref(),
            subscription.list.as_deref(),
            viewer,
            &mut subscription.state,
        )
        .await
        {
            Ok(events) => events,
            Err(error) => {
                console_error!(
                    "websocket streaming poll failed stream={} tag={} list={} error={}",
                    subscription.stream_name,
                    subscription.tag.as_deref().unwrap_or(""),
                    subscription.list.as_deref().unwrap_or(""),
                    error
                );
                if streaming_error_is_subrequest_limit(&error) {
                    return false;
                }
                continue;
            }
        };
        for event in events {
            let message = match streaming_websocket_event_message(subscription, &event) {
                Ok(message) => message,
                Err(error) => {
                    console_error!(
                        "websocket streaming event serialization failed stream={} error={}",
                        subscription.stream_name,
                        error
                    );
                    continue;
                }
            };
            if websocket.send_with_str(message).is_err() {
                return false;
            }
        }
    }
    true
}

async fn run_streaming_websocket(
    websocket: WebSocket,
    db: D1Database,
    config: cfwdon_core::AppConfig,
    initial_stream: Option<String>,
    initial_tag: Option<String>,
    initial_list: Option<String>,
    viewer: Option<crate::LocalAccount>,
) {
    let mut subscriptions = HashMap::<String, StreamingWebSocketSubscription>::new();
    if let Some(stream_name) = initial_stream {
        add_streaming_websocket_subscription(
            &mut subscriptions,
            stream_name,
            initial_tag,
            initial_list,
        );
    }
    {
        let mut websocket_events = match websocket.events() {
            Ok(events) => events,
            Err(error) => {
                console_error!("failed to attach websocket event stream: {}", error);
                let _ = websocket.close(Some(1011), Some("stream failed"));
                return;
            }
        };
        let mut poll_rounds = 0_u32;
        let mut subscription_polls = 0_u32;
        loop {
            let tick =
                worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).fuse();
            pin_mut!(tick);
            select! {
                event = websocket_events.next().fuse() => {
                    match event {
                        Some(Ok(WebsocketEvent::Message(message))) => {
                            let Some(text) = message.text() else {
                                let _ = websocket.send_with_str(streaming_websocket_error_message(
                                    "Only text websocket messages are supported",
                                    400,
                                ));
                                continue;
                            };
                            if !handle_streaming_websocket_client_message(
                                &websocket,
                                &mut subscriptions,
                                &text,
                                viewer.as_ref(),
                            ) {
                                break;
                            }
                        }
                        Some(Ok(WebsocketEvent::Close(_))) | None => break,
                        Some(Err(error)) => {
                            console_error!("websocket stream failed: {}", error);
                            break;
                        }
                    }
                }
                _ = tick => {
                    if subscriptions.is_empty() {
                        continue;
                    }
                    poll_rounds = poll_rounds.saturating_add(1);
                    subscription_polls =
                        subscription_polls.saturating_add(subscriptions.len() as u32);
                    if !poll_streaming_websocket_subscriptions(
                        &websocket,
                        &db,
                        &config,
                        viewer.as_ref(),
                        &mut subscriptions,
                    )
                    .await
                    {
                        break;
                    }
                    if streaming_poll_budget_exhausted(poll_rounds, subscription_polls) {
                        console_log!(
                            "websocket streaming recycled before subrequest limit rounds={} subscription_polls={}",
                            poll_rounds,
                            subscription_polls
                        );
                        break;
                    }
                }
            }
        }
    }
    let _ = websocket.close(Some(1000), Some("stream closed"));
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
    let websocket_protocol_token = if wants_websocket {
        websocket_protocol_access_token(&req)?
    } else {
        None
    };
    let extra_path = ctx
        .param("any")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let initial_stream = if wants_websocket && query.stream.is_none() && extra_path.is_none() {
        None
    } else {
        match validate_streaming_channel_request(
            query.stream.as_deref(),
            query.tag.as_deref(),
            query.list.as_deref(),
            extra_path,
        ) {
            Ok(stream) => Some(stream),
            Err(error) => return streaming_bad_request_response(error),
        }
    };
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let authenticated = match authenticate_local_api_request(&req, &db, &config).await? {
        LocalApiAuthentication::Auth0(viewer) => Some(viewer),
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
                .or(websocket_protocol_token.clone());
            match token {
                Some(token) => {
                    let Some(auth) =
                        find_oauth_access_token_with_account_by_bearer_token(&db, &token).await?
                    else {
                        return invalid_access_token_response();
                    };
                    if !oauth_access_token_has_any_scope(
                        &auth.token,
                        &["read", "read:statuses", "read:notifications"],
                    ) {
                        return invalid_access_token_response();
                    }
                    auth.account
                }
                None => None,
            }
        }
    };

    if initial_stream
        .as_deref()
        .is_some_and(streaming_channel_requires_auth)
        && authenticated.is_none()
    {
        return invalid_access_token_response();
    }

    if wants_websocket {
        let pair = WebSocketPair::new()?;
        pair.server.accept()?;
        let websocket = pair.server.clone();
        let db_for_ws = db;
        let config_for_ws = config.clone();
        let stream_name = initial_stream.clone();
        let tag_for_ws = query.tag.clone();
        let list_for_ws = query.list.clone();
        let viewer_for_ws = authenticated.clone();
        spawn_local(async move {
            run_streaming_websocket(
                websocket,
                db_for_ws,
                config_for_ws,
                stream_name,
                tag_for_ws,
                list_for_ws,
                viewer_for_ws,
            )
            .await;
        });
        let mut response = Response::from_websocket(pair.client)?;
        if let Some(protocol) = websocket_protocol_token.as_deref() {
            response
                .headers_mut()
                .set("Sec-WebSocket-Protocol", protocol)?;
        }
        return Ok(response);
    }

    let Some(stream) = initial_stream else {
        return streaming_bad_request_response(
            StreamingChannelValidationError::UnknownChannelRequested,
        );
    };

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

pub(crate) async fn status_quotes_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: QuotesQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
    let query_limit = timeline_fetch_limit(limit);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;

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
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            status.object_uri
        }
    };

    let local_quotes =
        list_local_status_quotes_by_uri(&db, &target_uri, &cursor, query_limit).await?;
    let remote_quotes =
        list_remote_status_quotes_by_uri(&db, &target_uri, &cursor, query_limit).await?;
    let local_status_ids = local_quotes
        .iter()
        .map(|quote| quote.id.clone())
        .collect::<Vec<_>>();
    let local_account_ids = local_quotes
        .iter()
        .map(|quote| quote.account_id.clone())
        .collect::<Vec<_>>();
    let remote_status_ids = remote_quotes
        .iter()
        .map(|quote| quote.id.clone())
        .collect::<Vec<_>>();
    let remote_actor_uris = remote_quotes
        .iter()
        .map(|quote| quote.actor_uri.clone())
        .collect::<Vec<_>>();
    let quote_uris = local_quotes
        .iter()
        .map(local_status_target_uri)
        .chain(remote_quotes.iter().map(|quote| quote.object_uri.clone()))
        .collect::<Vec<_>>();
    let local_quote_refs = local_quotes.iter().collect::<Vec<_>>();

    let (
        local_accounts_by_id,
        mut local_media_by_status_id,
        local_in_reply_to_account_ids,
        counts_preload,
        quote_counts_preload,
        local_poll_preload,
        local_viewer_state_preload,
        application_preload,
        remote_actors_by_uri,
        mut remote_attachments_by_status_id,
        remote_poll_preload,
        remote_edit_updated_at_preload,
    ) = futures_util::try_join!(
        find_accounts_by_ids(&db, &local_account_ids),
        find_media_attachments_by_status_ids(&db, &local_status_ids),
        load_in_reply_to_account_ids(&db, &local_quotes),
        preload_status_counts(&db, &local_status_ids, &remote_status_ids),
        preload_status_quote_counts(&db, &quote_uris),
        preload_mastodon_poll_responses(&db, &local_status_ids, viewer.as_ref()),
        async {
            match viewer.as_ref() {
                Some(viewer) => {
                    preload_local_status_viewer_state(&db, viewer.id(), &local_quote_refs, None)
                        .await
                }
                None => Ok(Default::default()),
            }
        },
        preload_status_applications(&db, &config, &local_quote_refs),
        find_remote_actors_by_actor_uris(&db, &remote_actor_uris),
        find_remote_status_attachments_by_status_ids(&db, &remote_status_ids),
        preload_remote_mastodon_poll_responses(&db, &remote_status_ids, viewer.as_ref()),
        preload_remote_status_edit_updated_at(&db, &remote_status_ids),
    )?;
    let remote_quote_refs = remote_quotes
        .iter()
        .filter_map(|quote| {
            remote_actors_by_uri
                .get(&quote.actor_uri)
                .map(|actor| (quote, actor))
        })
        .collect::<Vec<_>>();
    let remote_viewer_state_preload = match viewer.as_ref() {
        Some(viewer) => {
            preload_remote_status_viewer_state(&db, viewer.id(), &remote_quote_refs).await?
        }
        None => Default::default(),
    };

    let mut quotes: Vec<(String, String, serde_json::Value)> = Vec::new();

    for quote in local_quotes {
        let Some(account) = local_accounts_by_id.get(&quote.account_id) else {
            continue;
        };
        if !can_view_local_status(&db, &quote, viewer.as_ref(), account).await? {
            continue;
        }
        let media = local_media_by_status_id
            .remove(&quote.id)
            .unwrap_or_default();
        quotes.push((
            quote.created_at.clone(),
            quote.id.clone(),
            serde_json::to_value(
                build_local_status_response_with_quote_count_preloads(
                    &db,
                    &config,
                    viewer.as_ref(),
                    &quote,
                    account,
                    local_in_reply_to_account_ids.get(&quote.id).cloned(),
                    media,
                    None,
                    Some(&counts_preload),
                    Some(&quote_counts_preload),
                    Some(&local_poll_preload),
                    Some(&local_viewer_state_preload),
                    Some(&application_preload),
                )
                .await?,
            )?,
        ));
    }

    for quote in remote_quotes {
        if !is_public_activitypub_visibility(quote.visibility.as_str()) {
            continue;
        }
        let Some(actor) = remote_actors_by_uri.get(&quote.actor_uri) else {
            continue;
        };
        quotes.push((
            quote.published_at.clone(),
            quote.id.clone(),
            serde_json::to_value(
                build_remote_status_response_with_timeline_preloads(
                    &db,
                    &config,
                    viewer.as_ref(),
                    &quote,
                    actor,
                    None,
                    Some(&counts_preload),
                    Some(&quote_counts_preload),
                    Some(&remote_viewer_state_preload),
                    Some(&remote_poll_preload),
                    Some(&remote_edit_updated_at_preload),
                    remote_attachments_by_status_id
                        .remove(&quote.id)
                        .unwrap_or_default(),
                )
                .await?,
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
            requester.id(),
            target_status_id,
            &payload,
            &unique_follower_inboxes,
        )
        .await?;
    }
    if let Some(actor_uri) = remote_quote_author_actor_uri {
        let _ = queue_remote_actor_activity(db, requester.id(), actor_uri, &payload).await?;
    }

    Ok(())
}

async fn enqueue_quote_approval_federation(
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
    let authorization_uri = quote_authorization_uri(target_uri, authorization_key);
    let create_payload = build_create_quote_authorization_activity(
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
            requester.id(),
            target_status_id,
            &create_payload,
            &unique_follower_inboxes,
        )
        .await?;
    }

    if let Some(remote_actor_uri) = remote_quote_author_actor_uri {
        let quote_request = build_quote_request_object(
            &quote_request_uri(interacting_object_uri, authorization_key),
            remote_actor_uri,
            target_uri,
            interacting_object_uri,
        );
        let accept_payload = build_accept_quote_request_activity(
            config,
            requester,
            &quote_request,
            &authorization_uri,
            remote_actor_uri,
        )?;
        let _ = queue_remote_actor_activity(db, requester.id(), remote_actor_uri, &accept_payload)
            .await?;
    }

    Ok(())
}

async fn enqueue_quote_rejection_federation(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    target_uri: &str,
    interacting_object_uri: &str,
    authorization_key: &str,
    remote_quote_author_actor_uri: &str,
) -> Result<()> {
    let quote_request = build_quote_request_object(
        &quote_request_uri(interacting_object_uri, authorization_key),
        remote_quote_author_actor_uri,
        target_uri,
        interacting_object_uri,
    );
    let reject_payload = build_reject_quote_request_activity(
        config,
        requester,
        &quote_request,
        remote_quote_author_actor_uri,
    )?;
    let _ = queue_remote_actor_activity(
        db,
        requester.id(),
        remote_quote_author_actor_uri,
        &reject_payload,
    )
    .await?;

    Ok(())
}

async fn enqueue_quote_owner_decision_federation(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    action: OwnerQuoteAction,
    target_status_id: &str,
    target_uri: &str,
    interacting_object_uri: &str,
    authorization_key: &str,
    local_quote_author: Option<&cfwdon_domain::LocalAccount>,
    remote_quote_author_actor_uri: Option<&str>,
) -> Result<()> {
    match action {
        OwnerQuoteAction::Approve => {
            let follower_inboxes = list_follower_delivery_targets(db, requester.id()).await?;
            enqueue_quote_approval_federation(
                db,
                config,
                requester,
                target_status_id,
                target_uri,
                interacting_object_uri,
                authorization_key,
                &follower_inboxes,
                remote_quote_author_actor_uri,
            )
            .await?;
            if let Some(quote_author) = local_quote_author {
                let Some(updated_quote) = find_status_by_id(db, authorization_key).await? else {
                    return Ok(());
                };
                enqueue_status_update_activity(db, config, quote_author, &updated_quote).await?;
            }
        }
        OwnerQuoteAction::Reject => {
            if let Some(remote_actor_uri) = remote_quote_author_actor_uri {
                enqueue_quote_rejection_federation(
                    db,
                    config,
                    requester,
                    target_uri,
                    interacting_object_uri,
                    authorization_key,
                    remote_actor_uri,
                )
                .await?;
            } else if let Some(quote_author) = local_quote_author {
                let Some(updated_quote) = find_status_by_id(db, authorization_key).await? else {
                    return Ok(());
                };
                enqueue_status_update_activity(db, config, quote_author, &updated_quote).await?;
            }
        }
        OwnerQuoteAction::Revoke => {}
    }

    Ok(())
}

pub(crate) async fn approve_quote_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    quote_owner_action_response(req, ctx, OwnerQuoteAction::Approve).await
}

pub(crate) async fn reject_quote_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    quote_owner_action_response(req, ctx, OwnerQuoteAction::Reject).await
}

async fn quote_owner_action_response(
    req: Request,
    ctx: RouteContext<()>,
    action: OwnerQuoteAction,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let requester = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
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
    let (target_uri, target_status_id) = match &target_status {
        crate::ResolvedStatus::Local(status) => {
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                return Response::error("status not found", 404);
            };
            if status.account_id != requester.id()
                || !can_view_local_status(&db, status, Some(&requester), &account).await?
            {
                return Response::error("status not found", 404);
            }
            (local_status_target_uri(status), status.id.clone())
        }
        crate::ResolvedStatus::Remote(_) => return Response::error("status not found", 404),
    };

    match resolve_status_reference(&db, &config, &quote_status_id).await? {
        Some(crate::ResolvedStatus::Local(quote_status)) => {
            if quote_status.quote_of_uri.as_deref() != Some(target_uri.as_str())
                || quote_status.effective_quote_state() != QuoteState::Pending
            {
                return Response::error("status not found", 404);
            }
            let Some(quote_author) = find_account_by_id(&db, &quote_status.account_id).await?
            else {
                return Response::error("status not found", 404);
            };
            let updated_at = now_iso_string()?;
            let updated_status = update_local_status_quote_state(
                &db,
                &quote_status,
                quote_status
                    .quote_state
                    .quote_state_after_owner_action(action),
                &updated_at,
            )
            .await?;
            enqueue_quote_owner_decision_federation(
                &db,
                &config,
                &requester,
                action,
                &target_status_id,
                &target_uri,
                &local_status_target_uri(&updated_status),
                &updated_status.id,
                Some(&quote_author),
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
                || quote_status.effective_quote_state() != QuoteState::Pending
            {
                return Response::error("status not found", 404);
            }
            let Some(quote_author) =
                find_remote_actor_by_actor_uri(&db, &quote_status.actor_uri).await?
            else {
                return Response::error("status not found", 404);
            };
            let updated_status = update_remote_status_quote_state(
                &db,
                &quote_status.id,
                quote_status
                    .quote_state
                    .quote_state_after_owner_action(action),
            )
            .await?;
            enqueue_quote_owner_decision_federation(
                &db,
                &config,
                &requester,
                action,
                &target_status_id,
                &target_uri,
                &quote_status.object_uri,
                &quote_status.id,
                None,
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

pub(crate) async fn revoke_quote_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let requester = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
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
            (status.id.clone(), local_status_target_uri(status))
        }
        crate::ResolvedStatus::Remote(status) => (status.id.clone(), status.object_uri.clone()),
    };

    match resolve_status_reference(&db, &config, &quote_status_id).await? {
        Some(crate::ResolvedStatus::Local(quote_status)) => {
            if !local_quote_revoke_allowed(requester.id(), &quote_status, target_uri.as_str()) {
                return Response::error("status not found", 404);
            }
            let Some(quote_author) = find_account_by_id(&db, &quote_status.account_id).await?
            else {
                return Response::error("status not found", 404);
            };
            let quote_revocation_targets =
                list_follower_delivery_targets(&db, quote_author.id()).await?;

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
        Some(crate::ResolvedStatus::Remote(_)) => Response::error("status not found", 404),
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
                let stats = load_account_stats(&db, account.id()).await?;
                response.push(crate::MastodonAccountResponse::from_account_with_stats(
                    &account, &config, &stats,
                ));
            }
            Some(AccountReference::Remote(actor)) => {
                let fetched =
                    crate::fetch_remote_actor_profile_with_document(&actor.actor_uri).await;
                let mut account = match fetched.as_ref() {
                    Ok(fetched) => {
                        let _ = crate::upsert_remote_actor(&db, &fetched.profile).await;
                        crate::MastodonAccountResponse::from_remote_actor_profile(&fetched.profile)
                    }
                    Err(_) => crate::MastodonAccountResponse::from_remote_actor(&actor),
                };
                if let Ok(fetched) = fetched
                    && let Ok(counts) =
                        crate::load_remote_actor_social_counts_from_document(&fetched.document)
                            .await
                {
                    crate::apply_remote_actor_social_counts(&mut account, counts);
                }
                if let Ok(summary) =
                    crate::load_remote_actor_status_summary(&db, &account.uri).await
                {
                    if summary.statuses_count > 0 {
                        account.statuses_count = summary.statuses_count;
                    }
                    account.last_status_at = summary.last_status_at;
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
    let agreement = parse_optional_bool(request.agreement.as_deref()).unwrap_or_default();
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
    result
        .results::<crate::StatusRecord>()
        .map(|rows| rows.into_iter().map(crate::status_from_record).collect())
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
    result.results::<crate::RemoteStatusRecord>().map(|rows| {
        rows.into_iter()
            .map(crate::remote_status_from_record)
            .collect()
    })
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

    #[test]
    fn streaming_websocket_event_message_matches_mastodon_shape() {
        let subscription =
            StreamingWebSocketSubscription::new("user:notification".to_owned(), None, None);
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "notification-1".to_owned(),
            event: "notification",
            data: "{\"id\":\"notification-1\"}".to_owned(),
        };

        let message = streaming_websocket_event_message(&subscription, &event).unwrap();
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(value["stream"], serde_json::json!(["user:notification"]));
        assert_eq!(value["event"], "notification");
        assert_eq!(value["payload"], "{\"id\":\"notification-1\"}");
    }

    #[test]
    fn streaming_websocket_filter_change_omits_payload() {
        let subscription = StreamingWebSocketSubscription::new("user".to_owned(), None, None);
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "filter-change".to_owned(),
            event: "filters_changed",
            data: "undefined".to_owned(),
        };

        let message = streaming_websocket_event_message(&subscription, &event).unwrap();
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(value["stream"], serde_json::json!(["user"]));
        assert_eq!(value["event"], "filters_changed");
        assert!(value.get("payload").is_none());
    }

    #[test]
    fn streaming_websocket_stream_labels_include_subscription_params() {
        assert_eq!(
            streaming_websocket_stream_labels("hashtag", Some("rust"), None),
            vec!["hashtag".to_owned(), "rust".to_owned()]
        );
        assert_eq!(
            streaming_websocket_stream_labels("list", None, Some("list-1")),
            vec!["list".to_owned(), "list-1".to_owned()]
        );
    }

    #[test]
    fn streaming_channel_validation_accepts_query_and_path_channels() {
        assert_eq!(
            validate_streaming_channel_request(Some("public"), None, None, None).unwrap(),
            "public"
        );
        assert_eq!(
            validate_streaming_channel_request(None, None, None, Some("user")).unwrap(),
            "user"
        );
        assert_eq!(
            validate_streaming_channel_request(None, None, None, Some("public/local/media"))
                .unwrap(),
            "public:local:media"
        );
        assert_eq!(
            validate_streaming_channel_request(None, Some("rust"), None, Some("hashtag")).unwrap(),
            "hashtag"
        );
    }

    #[test]
    fn streaming_channel_validation_rejects_conflicting_query_and_path_channels() {
        assert!(matches!(
            validate_streaming_channel_request(Some("public"), None, None, Some("user")),
            Err(StreamingChannelValidationError::UnknownChannelRequested)
        ));
    }

    #[test]
    fn streaming_filter_update_changed_only_after_initial_state() {
        assert!(!streaming_filter_update_changed(
            None,
            "2026-05-01T00:00:00Z"
        ));
        assert!(!streaming_filter_update_changed(
            Some("2026-05-01T00:00:00Z"),
            "2026-05-01T00:00:00Z"
        ));
        assert!(streaming_filter_update_changed(
            Some("2026-05-01T00:00:00Z"),
            "2026-05-02T00:00:00Z"
        ));
    }

    #[test]
    fn announcement_stream_entry_action_prioritizes_reaction_delta() {
        let previous_reactions = BTreeMap::from([("wave".to_owned(), (1, false))]);
        let current_reactions = BTreeMap::from([("wave".to_owned(), (2, true))]);

        assert_eq!(
            announcement_stream_entry_action(
                true,
                Some("{\"id\":\"announcement-1\"}"),
                "{\"id\":\"announcement-2\"}",
                &current_reactions,
                &previous_reactions,
            ),
            AnnouncementStreamEntryAction::None
        );
        assert_eq!(
            announcement_stream_entry_action(
                false,
                Some("{\"id\":\"announcement-1\"}"),
                "{\"id\":\"announcement-2\"}",
                &current_reactions,
                &previous_reactions,
            ),
            AnnouncementStreamEntryAction::Reaction
        );
    }

    #[test]
    fn announcement_stream_entry_action_detects_payload_delta_after_reactions() {
        let reactions = BTreeMap::from([("wave".to_owned(), (1, false))]);

        assert_eq!(
            announcement_stream_entry_action(
                false,
                Some("{\"id\":\"announcement-1\"}"),
                "{\"id\":\"announcement-2\"}",
                &reactions,
                &reactions,
            ),
            AnnouncementStreamEntryAction::Announcement
        );
        assert_eq!(
            announcement_stream_entry_action(
                false,
                Some("{\"id\":\"announcement-1\"}"),
                "{\"id\":\"announcement-1\"}",
                &reactions,
                &reactions,
            ),
            AnnouncementStreamEntryAction::None
        );
    }

    #[test]
    fn append_current_announcement_stream_entry_events_prioritizes_reaction_events() {
        let entry = AnnouncementStreamEntry {
            id: "announcement-1".to_owned(),
            payload: "{\"id\":\"announcement-1\",\"content\":\"new\"}".to_owned(),
            created_at: "2026-05-01T00:00:00Z".to_owned(),
        };
        let previous_announcements = HashMap::from([(
            "announcement-1".to_owned(),
            "{\"id\":\"announcement-1\",\"content\":\"old\"}".to_owned(),
        )]);
        let previous_reactions =
            HashMap::from([(("announcement-1".to_owned(), "wave".to_owned()), (1, false))]);
        let current_reactions =
            HashMap::from([(("announcement-1".to_owned(), "wave".to_owned()), (2, true))]);
        let mut events = Vec::new();

        append_current_announcement_stream_entry_events(
            &entry,
            false,
            &previous_announcements,
            &previous_reactions,
            &current_reactions,
            &mut events,
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "announcement.reaction");
        assert_eq!(events[0].id, "announcement-1:wave");
    }

    #[test]
    fn removed_announcement_ids_returns_only_missing_current_ids() {
        let previous = HashMap::from([
            ("announcement-1".to_owned(), "{}".to_owned()),
            ("announcement-2".to_owned(), "{}".to_owned()),
        ]);
        let current = HashMap::from([("announcement-2".to_owned(), "{}".to_owned())]);

        assert_eq!(
            removed_announcement_ids(&previous, &current),
            vec!["announcement-1".to_owned()]
        );
    }

    #[test]
    fn required_streaming_list_id_rejects_missing_or_blank_values() {
        assert!(required_streaming_list_id(None).is_err());
        assert!(required_streaming_list_id(Some("   ")).is_err());
        assert_eq!(
            required_streaming_list_id(Some("list-1")).unwrap(),
            "list-1"
        );
    }

    #[test]
    fn streaming_event_key_combines_event_type_and_id() {
        let event = StreamingEvent {
            created_at: "2026-05-01T00:00:00Z".to_owned(),
            id: "status-1".to_owned(),
            event: "update",
            data: "{}".to_owned(),
        };

        assert_eq!(streaming_event_key(&event), "update:status-1");
    }

    #[test]
    fn streaming_status_delta_already_recorded_skips_deleted_or_updated_ids() {
        let deleted_status_ids = HashSet::from(["deleted-1".to_owned()]);
        let updated_status_ids = HashSet::from(["updated-1".to_owned()]);

        assert!(streaming_status_delta_already_recorded(
            "deleted-1",
            &deleted_status_ids,
            &updated_status_ids
        ));
        assert!(streaming_status_delta_already_recorded(
            "updated-1",
            &deleted_status_ids,
            &updated_status_ids
        ));
        assert!(!streaming_status_delta_already_recorded(
            "fresh-1",
            &deleted_status_ids,
            &updated_status_ids
        ));
    }

    #[test]
    fn streaming_status_delete_event_matches_mastodon_delete_shape() {
        let event = streaming_status_delete_event("status-1", "2026-05-01T00:00:00Z".to_owned());

        assert_eq!(event.created_at, "2026-05-01T00:00:00Z");
        assert_eq!(event.id, "status-1");
        assert_eq!(event.event, "delete");
        assert_eq!(event.data, "status-1");
    }

    #[test]
    fn list_stream_membership_refs_include_any_accepts_any_candidate_variant() {
        let membership_refs = HashSet::from(["alice@example.com".to_owned()]);

        assert!(list_stream_membership_refs_include_any(
            &membership_refs,
            vec![
                "acct:alice@example.com".to_owned(),
                "alice@example.com".to_owned()
            ]
        ));
        assert!(!list_stream_membership_refs_include_any(
            &membership_refs,
            vec!["bob@example.com".to_owned()]
        ));
    }

    #[test]
    fn list_stream_status_policy_requires_membership_and_allowed_reply() {
        let membership_refs = HashSet::from(["alice@example.com".to_owned()]);
        let allow_replies = ListStreamStatusPolicy::new(&membership_refs, "list");
        let exclude_replies = ListStreamStatusPolicy::new(&membership_refs, "none");

        assert!(allow_replies.matches(
            vec![
                "acct:alice@example.com".to_owned(),
                "alice@example.com".to_owned()
            ],
            Some("status-1"),
        ));
        assert!(!allow_replies.matches(vec!["bob@example.com".to_owned()], None,));
        assert!(!exclude_replies.matches(vec!["alice@example.com".to_owned()], Some("status-1"),));
    }

    #[test]
    fn list_stream_excludes_reply_only_when_policy_blocks_replies() {
        assert!(list_stream_excludes_reply("none", Some("status-1")));
        assert!(!list_stream_excludes_reply("list", Some("status-1")));
        assert!(!list_stream_excludes_reply("none", None));
    }

    #[test]
    fn announcement_stream_entry_extracts_payload_identity_and_time() {
        let announcement = serde_json::json!({
            "id": "announcement-1",
            "published_at": "2026-05-01T00:00:00Z",
            "content": "<p>Hello</p>"
        });

        let entry = announcement_stream_entry(&announcement).unwrap().unwrap();

        assert_eq!(entry.id, "announcement-1");
        assert_eq!(entry.created_at, "2026-05-01T00:00:00Z");
        assert!(entry.payload.contains("\"announcement-1\""));
    }

    #[test]
    fn announcement_stream_entry_uses_updated_at_fallback() {
        let announcement = serde_json::json!({
            "id": "announcement-1",
            "updated_at": "2026-05-02T00:00:00Z",
        });

        let entry = announcement_stream_entry(&announcement).unwrap().unwrap();

        assert_eq!(entry.created_at, "2026-05-02T00:00:00Z");
    }

    #[test]
    fn announcement_stream_entries_skips_documents_without_stream_identity() {
        let announcements = vec![
            serde_json::json!({
                "id": "announcement-1",
                "published_at": "2026-05-01T00:00:00Z",
            }),
            serde_json::json!({
                "content": "<p>missing id</p>",
            }),
        ];

        let entries = announcement_stream_entries(announcements).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "announcement-1");
    }

    #[test]
    fn streaming_poll_budget_exhausts_before_cloudflare_subrequest_limit() {
        assert!(!streaming_poll_budget_exhausted(
            STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION - 1,
            STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION - 1
        ));
        assert!(streaming_poll_budget_exhausted(
            STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION,
            1
        ));
        assert!(streaming_poll_budget_exhausted(
            1,
            STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION
        ));
    }

    #[test]
    fn streaming_error_detects_cloudflare_subrequest_limit() {
        let error = worker::Error::RustError(
            "Error: Too many API requests by single Worker invocation".to_owned(),
        );

        assert!(streaming_error_is_subrequest_limit(&error));
    }
}
