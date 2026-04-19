use crate::{
    AccountReference, Request, Response, Result, RouteContext, TimelinePaginationQuery, actor_url,
    build_local_status_response, build_relationship_for_target, build_remote_status_response,
    build_timeline_link_header, can_view_local_status, clear_local_status_quote,
    delete_follow_by_target, delete_follower_by_actor, enqueue_status_update_activity,
    extract_authenticated_user, find_account_by_id, find_authenticated_local_account,
    find_local_status_by_object_uri, find_media_attachments_by_status_id,
    find_remote_actor_by_actor_uri, find_status_by_id, first_url_from_text, generate_entity_id,
    insert_status_edit_snapshot, instance_base_url, is_public_activitypub_visibility,
    load_account_stats, load_config, load_in_reply_to_account_id, local_status_ap_id,
    local_status_target_uri, media_object_url, normalize_status_history_entry, now_iso_string,
    parse_relationship_query_ids, remote_account_rest_id, resolve_account_reference,
    resolve_local_account, resolve_status_reference, resolve_timeline_cursor, timeline_fetch_limit,
    timeline_limit,
};
use serde::Deserialize;
use worker::{ResponseBody, d1::D1Type};

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

fn oauth_base_url(config: &cfwdon_core::AppConfig) -> String {
    instance_base_url(config)
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
    let picture = account
        .avatar_object_key
        .as_deref()
        .map(|object_key| serde_json::json!(media_object_url(config, object_key)))
        .unwrap_or(serde_json::Value::Null);
    let website = account
        .fields
        .iter()
        .find_map(|field| first_url_from_text(&field.value))
        .map(serde_json::Value::String)
        .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "sub": account.id,
        "preferred_username": account.username,
        "name": account.display_name,
        "nickname": account.username,
        "profile": format!("{base_url}/@{}", account.username),
        "website": website,
        "picture": picture,
        "email": account.access_email,
        "email_verified": true,
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

pub(crate) fn build_app_verify_credentials_document(
    config: &cfwdon_core::AppConfig,
) -> serde_json::Value {
    serde_json::json!({
        "id": "0",
        "name": config.instance_name,
        "website": serde_json::Value::Null,
        "scopes": ["read"],
        "redirect_uris": ["urn:ietf:wg:oauth:2.0:oob"],
        "redirect_uri": "urn:ietf:wg:oauth:2.0:oob",
        "vapid_key": config.web_push_vapid_public_key.as_deref().unwrap_or(""),
    })
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

async fn is_authenticated_request(req: &Request, config: &cfwdon_core::AppConfig) -> Result<bool> {
    Ok(extract_authenticated_user(req, config).await?.is_some())
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
    if !is_authenticated_request(&req, &config).await? {
        return Response::error("Cloudflare Access authentication required", 401);
    }
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
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
    if !is_authenticated_request(&req, &config).await? {
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
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
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
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::error("annual report not found", 404);
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
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
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Ok(Response::empty()?);
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let url = req.url()?;

    if url.path().ends_with("/read") {
        mark_generated_annual_report_viewed(&db, &account.id, year).await?;
        return Ok(Response::empty()?);
    }

    if year != current_campaign_year() {
        return Ok(Response::empty()?);
    }
    if find_generated_annual_report(&db, &account.id, year)
        .await?
        .is_some()
    {
        return Ok(Response::empty()?);
    }

    create_generated_annual_report(&db, &account, year).await?;
    Ok(Response::empty()?.with_status(202))
}

pub(crate) async fn annual_report_state_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::from_json(&serde_json::json!({
            "state": "unavailable",
            "available": false,
        }));
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;

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
    if !is_authenticated_request(&req, &config).await? {
        return Response::error("Cloudflare Access authentication required", 401);
    }
    Response::from_json(&build_app_verify_credentials_document(&config))
}

pub(crate) async fn create_email_confirmation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    if !is_authenticated_request(&req, &config).await? {
        return Response::error("Cloudflare Access authentication required", 401);
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn check_email_confirmation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    if !is_authenticated_request(&req, &config).await? {
        return Response::error("Cloudflare Access authentication required", 401);
    }
    Response::from_json(&true)
}

pub(crate) async fn streaming_placeholder_response(
    _req: Request,
    _ctx: RouteContext<()>,
) -> Result<Response> {
    let mut response = Response::from_body(ResponseBody::Body(
        b": cfwdon-placeholder streaming endpoint\n\n".to_vec(),
    ))?;
    response
        .headers_mut()
        .set("Content-Type", "text/event-stream")?;
    Ok(response)
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
    let target_uri = match target_status {
        crate::ResolvedStatus::Local(status) => {
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                return Response::error("status not found", 404);
            };
            if !can_view_local_status(&db, &status, Some(&requester), &account).await? {
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

    let Some(quote_status) = find_status_by_id(&db, &quote_status_id).await? else {
        return Response::error("status not found", 404);
    };
    if quote_status.account_id != requester.id {
        return Response::error("status not found", 404);
    }
    if quote_status.quote_of_uri.as_deref() != Some(target_uri.as_str()) {
        return Response::error("status not found", 404);
    }

    let current_media = find_media_attachments_by_status_id(&db, &quote_status.id).await?;
    let previous_in_reply_to_account_id = load_in_reply_to_account_id(&db, &quote_status).await?;
    let previous_response = build_local_status_response(
        &db,
        &config,
        Some(&requester),
        &quote_status,
        &requester,
        previous_in_reply_to_account_id,
        current_media.clone(),
    )
    .await?;
    let mut previous_snapshot =
        serde_json::to_value(previous_response).unwrap_or_else(|_| serde_json::json!({}));
    let revision_at = now_iso_string()?;
    previous_snapshot["created_at"] = serde_json::json!(revision_at.clone());
    let previous_snapshot = normalize_status_history_entry(previous_snapshot);
    let previous_snapshot_json = serde_json::to_string(&previous_snapshot).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize status snapshot: {error}"))
    })?;
    insert_status_edit_snapshot(&db, &quote_status.id, &previous_snapshot_json, &revision_at)
        .await?;

    let updated_status = clear_local_status_quote(&db, &quote_status, &revision_at).await?;
    enqueue_status_update_activity(&db, &config, &requester, &updated_status).await?;

    let media = find_media_attachments_by_status_id(&db, &updated_status.id).await?;
    let in_reply_to_account_id = load_in_reply_to_account_id(&db, &updated_status).await?;
    let response = build_local_status_response(
        &db,
        &config,
        Some(&requester),
        &updated_status,
        &requester,
        in_reply_to_account_id,
        media,
    )
    .await?;

    Response::from_json(&response)
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
    _req: Request,
    _ctx: RouteContext<()>,
) -> Result<Response> {
    Response::from_json(&serde_json::json!({
        "id": "cfwdon-placeholder-account",
        "token": "",
        "access_token": serde_json::Value::Null,
    }))
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
    let bindings = [
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
    ];
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE quote_of_uri = ?1
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
    let bindings = [
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
    ];
    let result = db
        .prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
             FROM remote_statuses
             WHERE quote_of_uri = ?1
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
