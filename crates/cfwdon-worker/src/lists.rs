use crate::accounts::load_account_stats;
use crate::auth::{find_account_by_id, find_account_by_username};
use crate::id_utils::generate_entity_id;
use crate::instance::parse_lookup_handle;
use crate::media::find_media_attachments_by_status_id;
use crate::profile::require_authenticated_local_account;
use crate::relationship::is_muted_actor;
use crate::remote::{
    AccountReference, find_remote_actor_by_actor_uri, find_remote_actor_by_username_domain,
    resolve_account_reference,
};
use crate::runtime_config::load_config;
use crate::statuses::{
    build_local_status_response, build_remote_status_response, is_local_status_thread_muted_by,
    list_local_public_timeline_statuses, list_remote_public_timeline_statuses,
    load_in_reply_to_account_id,
};
use crate::timelines::{
    TimelinePaginationQuery, build_timeline_link_header, resolve_timeline_cursor,
    timeline_fetch_limit, timeline_limit,
};
use serde::Deserialize;
use std::collections::HashSet;
use worker::d1::D1Type;
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Deserialize)]
pub(crate) struct AccountListRow {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) replies_policy: String,
    pub(crate) exclusive: i32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccountListMembershipRow {
    pub(crate) target_account_ref: String,
}

#[derive(Debug, Default, Deserialize)]
struct ListRequest {
    title: Option<String>,
    replies_policy: Option<String>,
    exclusive: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct ListAccountsRequest {
    account_ids: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct ListTimelineQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

impl ListTimelineQuery {
    fn pagination(&self) -> TimelinePaginationQuery {
        TimelinePaginationQuery {
            limit: self.limit,
            max_id: self.max_id.clone(),
            since_id: self.since_id.clone(),
            min_id: self.min_id.clone(),
        }
    }
}

fn normalize_replies_policy(value: Option<&str>) -> std::result::Result<String, String> {
    let normalized = value.unwrap_or("list").trim().to_ascii_lowercase();
    match normalized.as_str() {
        "followed" | "list" | "none" => Ok(normalized),
        _ => Err("replies_policy must be one of: followed, list, none".to_owned()),
    }
}

fn parse_form_bool(value: Option<String>) -> Option<bool> {
    match value.as_deref().map(str::trim) {
        Some("true" | "1" | "on") => Some(true),
        Some("false" | "0" | "off") => Some(false),
        _ => None,
    }
}

async fn parse_list_request(req: &mut Request) -> std::result::Result<ListRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<ListRequest>()
            .await
            .map_err(|error| format!("invalid list JSON payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid list form payload: {error}"))?;
        ListRequest {
            title: form.get_field("title"),
            replies_policy: form.get_field("replies_policy"),
            exclusive: parse_form_bool(form.get_field("exclusive")),
        }
    };

    if let Some(title) = request.title.as_mut() {
        *title = title.trim().to_owned();
    }
    if request.title.as_deref().unwrap_or_default().is_empty() {
        return Err("title must not be empty".to_owned());
    }
    request.replies_policy = Some(normalize_replies_policy(request.replies_policy.as_deref())?);
    Ok(request)
}

async fn parse_list_accounts_request(
    req: &mut Request,
) -> std::result::Result<ListAccountsRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        req.json::<ListAccountsRequest>()
            .await
            .map_err(|error| format!("invalid list accounts JSON payload: {error}"))
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid list accounts form payload: {error}"))?;
        Ok(ListAccountsRequest {
            account_ids: form.get_all("account_ids[]").map(|ids| {
                ids.into_iter()
                    .filter_map(|entry| match entry {
                        worker::FormEntry::Field(value) => Some(value),
                        worker::FormEntry::File(_) => None,
                    })
                    .collect()
            }),
        })
    }
}

async fn list_rows_for_account(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<Vec<AccountListRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT id, title, replies_policy, exclusive
             FROM account_lists
             WHERE account_id = ?1
             ORDER BY created_at DESC, id DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<AccountListRow>()
}

pub(crate) async fn list_row_by_id(
    db: &worker::D1Database,
    account_id: &str,
    list_id: &str,
) -> Result<Option<AccountListRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(list_id)];
    db.prepare(
        "SELECT id, title, replies_policy, exclusive
         FROM account_lists
         WHERE account_id = ?1
           AND id = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<AccountListRow>(None)
    .await
}

fn list_document(row: &AccountListRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "title": row.title,
        "replies_policy": row.replies_policy,
        "exclusive": row.exclusive != 0,
    })
}

async fn create_list_row(
    db: &worker::D1Database,
    account_id: &str,
    request: &ListRequest,
) -> Result<AccountListRow> {
    let list_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(list_id.as_str()),
        D1Type::Text(account_id),
        D1Type::Text(request.title.as_deref().unwrap_or_default()),
        D1Type::Text(request.replies_policy.as_deref().unwrap_or("list")),
        D1Type::Integer(if request.exclusive.unwrap_or(false) {
            1
        } else {
            0
        }),
    ];
    db.prepare(
        "INSERT INTO account_lists (id, account_id, title, replies_policy, exclusive)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    list_row_by_id(db, account_id, &list_id)
        .await?
        .ok_or_else(|| worker::Error::RustError("failed to reload created list".to_owned()))
}

async fn update_list_row(
    db: &worker::D1Database,
    account_id: &str,
    list_id: &str,
    request: &ListRequest,
) -> Result<Option<AccountListRow>> {
    let existing = list_row_by_id(db, account_id, list_id).await?;
    let Some(existing) = existing else {
        return Ok(None);
    };
    let bindings = [
        D1Type::Text(request.title.as_deref().unwrap_or_default()),
        D1Type::Text(request.replies_policy.as_deref().unwrap_or("list")),
        D1Type::Integer(if request.exclusive.unwrap_or(existing.exclusive != 0) {
            1
        } else {
            0
        }),
        D1Type::Text(account_id),
        D1Type::Text(list_id),
    ];
    db.prepare(
        "UPDATE account_lists
         SET title = ?1,
             replies_policy = ?2,
             exclusive = ?3,
             updated_at = CURRENT_TIMESTAMP
         WHERE account_id = ?4
           AND id = ?5",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    list_row_by_id(db, account_id, list_id).await
}

async fn delete_list_row(db: &worker::D1Database, account_id: &str, list_id: &str) -> Result<bool> {
    if list_row_by_id(db, account_id, list_id).await?.is_none() {
        return Ok(false);
    }
    let bindings = [D1Type::Text(account_id), D1Type::Text(list_id)];
    db.prepare(
        "DELETE FROM account_lists
         WHERE account_id = ?1
           AND id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(true)
}

pub(crate) async fn list_membership_refs(
    db: &worker::D1Database,
    list_id: &str,
) -> Result<Vec<AccountListMembershipRow>> {
    let list_id = D1Type::Text(list_id);
    let result = db
        .prepare(
            "SELECT target_account_ref
             FROM account_list_memberships
             WHERE list_id = ?1
             ORDER BY target_account_ref ASC",
        )
        .bind_refs(&list_id)?
        .all()
        .await?;
    result.results::<AccountListMembershipRow>()
}

async fn add_accounts_to_list(
    db: &worker::D1Database,
    list_id: &str,
    account_refs: &[String],
) -> Result<()> {
    for account_ref in account_refs {
        let trimmed = account_ref.trim();
        if trimmed.is_empty() {
            continue;
        }
        let bindings = [D1Type::Text(list_id), D1Type::Text(trimmed)];
        db.prepare(
            "INSERT INTO account_list_memberships (list_id, target_account_ref)
             VALUES (?1, ?2)
             ON CONFLICT(list_id, target_account_ref) DO NOTHING",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }
    Ok(())
}

async fn remove_accounts_from_list(
    db: &worker::D1Database,
    list_id: &str,
    account_refs: &[String],
) -> Result<()> {
    for account_ref in account_refs {
        let trimmed = account_ref.trim();
        if trimmed.is_empty() {
            continue;
        }
        let bindings = [D1Type::Text(list_id), D1Type::Text(trimmed)];
        db.prepare(
            "DELETE FROM account_list_memberships
             WHERE list_id = ?1
               AND target_account_ref = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }
    Ok(())
}

async fn resolve_list_member_document(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ref: &str,
) -> Result<Option<serde_json::Value>> {
    if let Some(account) = find_account_by_id(db, account_ref).await? {
        let stats = load_account_stats(db, &account.id).await?;
        return Ok(Some(serde_json::to_value(
            crate::MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
        )?));
    }
    if let Some(actor) = find_remote_actor_by_actor_uri(db, account_ref).await? {
        return Ok(Some(serde_json::to_value(
            crate::MastodonAccountResponse::from_remote_actor(&actor),
        )?));
    }
    if account_ref.contains('@') {
        let handle = parse_lookup_handle(account_ref, config).map_err(|error| {
            worker::Error::RustError(format!("invalid list member ref: {error}"))
        })?;
        if let Some(domain) = handle.domain.as_deref()
            && domain != config.instance_domain
            && let Some(actor) =
                find_remote_actor_by_username_domain(db, &handle.username, domain).await?
        {
            return Ok(Some(serde_json::to_value(
                crate::MastodonAccountResponse::from_remote_actor(&actor),
            )?));
        }
    }
    Ok(None)
}

fn list_id_from_context(ctx: &RouteContext<()>) -> Result<String> {
    ctx.param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing list id route parameter".to_owned()))
}

pub(crate) fn list_membership_variants_for_local_account(
    account: &cfwdon_domain::LocalAccount,
    config: &cfwdon_core::AppConfig,
) -> [String; 2] {
    [
        account.id.clone(),
        format!("{}@{}", account.username, config.instance_domain),
    ]
}

pub(crate) fn list_membership_variants_for_remote_actor(
    actor: &crate::RemoteActorRow,
) -> [String; 2] {
    [
        actor.actor_uri.clone(),
        format!("{}@{}", actor.username, actor.domain),
    ]
}

async fn requested_account_membership_variants(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ref: &str,
) -> Result<Option<Vec<String>>> {
    match resolve_account_reference(db, account_ref).await? {
        Some(AccountReference::Local(account)) => {
            return Ok(Some(
                list_membership_variants_for_local_account(&account, config).into(),
            ));
        }
        Some(AccountReference::Remote(actor)) => {
            return Ok(Some(
                list_membership_variants_for_remote_actor(&actor).into(),
            ));
        }
        None => {}
    }

    let handle = match parse_lookup_handle(account_ref, config) {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };

    if handle.is_local_to(&config.instance_domain) {
        if let Some(account) = find_account_by_username(db, &handle.username).await? {
            return Ok(Some(
                list_membership_variants_for_local_account(&account, config).into(),
            ));
        }
        return Ok(None);
    }

    let Some(domain) = handle.domain.as_deref() else {
        return Ok(None);
    };
    Ok(
        find_remote_actor_by_username_domain(db, &handle.username, domain)
            .await?
            .map(|actor| list_membership_variants_for_remote_actor(&actor).into()),
    )
}

pub(crate) async fn account_lists_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let account_ref = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(target_refs) =
        requested_account_membership_variants(&db, &config, &account_ref).await?
    else {
        return Response::error("account not found", 404);
    };

    let target_refs = target_refs.into_iter().collect::<HashSet<_>>();
    let mut documents = Vec::new();
    for row in list_rows_for_account(&db, &account.id).await? {
        let memberships = list_membership_refs(&db, &row.id).await?;
        if memberships
            .into_iter()
            .any(|membership| target_refs.contains(&membership.target_account_ref))
        {
            documents.push(list_document(&row));
        }
    }

    Response::from_json(&documents)
}

pub(crate) async fn list_timeline_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: ListTimelineQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
    let query_limit = timeline_fetch_limit(limit);
    let list_id = list_id_from_context(&ctx)?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(list) = list_row_by_id(&db, &account.id, &list_id).await? else {
        return Response::error("list not found", 404);
    };
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;
    let membership_refs = list_membership_refs(&db, &list_id)
        .await?
        .into_iter()
        .map(|row| row.target_account_ref)
        .collect::<HashSet<_>>();
    let mut entries = Vec::new();

    for status in list_local_public_timeline_statuses(&db, &cursor, query_limit).await? {
        let Some(author) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        if !list_membership_variants_for_local_account(&author, &config)
            .into_iter()
            .any(|candidate| membership_refs.contains(&candidate))
        {
            continue;
        }
        if list.replies_policy == "none" && status.in_reply_to_id.is_some() {
            continue;
        }
        if is_local_status_thread_muted_by(&db, &account.id, &status).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            status.id.clone(),
            build_local_status_response(
                &db,
                &config,
                Some(&account),
                &status,
                &author,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
            )
            .await?,
        ));
    }

    for (status, actor) in list_remote_public_timeline_statuses(&db, &cursor, query_limit).await? {
        if !list_membership_variants_for_remote_actor(&actor)
            .into_iter()
            .any(|candidate| membership_refs.contains(&candidate))
        {
            continue;
        }
        if list.replies_policy == "none" && status.in_reply_to_uri.is_some() {
            continue;
        }
        if is_muted_actor(&db, &account.id, &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            status.id.clone(),
            build_remote_status_response(&db, &config, Some(&account), &status, &actor).await?,
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let first_id = entries
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = entries
        .last()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let response = entries
        .into_iter()
        .map(|(_, _, status)| status)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let mut builder = Response::from_json(&response)?;
    if let Some(link) =
        build_timeline_link_header(&req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}

pub(crate) async fn lists_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let documents = list_rows_for_account(&db, &account.id)
        .await?
        .into_iter()
        .map(|row| list_document(&row))
        .collect::<Vec<_>>();
    Response::from_json(&documents)
}

pub(crate) async fn create_list_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let request = parse_list_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let row = create_list_row(&db, &account.id, &request).await?;
    Response::from_json(&list_document(&row))
}

pub(crate) async fn list_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let list_id = list_id_from_context(&ctx)?;
    match list_row_by_id(&db, &account.id, &list_id).await? {
        Some(row) => Response::from_json(&list_document(&row)),
        None => Response::error("list not found", 404),
    }
}

pub(crate) async fn update_list_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let list_id = list_id_from_context(&ctx)?;
    let request = parse_list_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    match update_list_row(&db, &account.id, &list_id, &request).await? {
        Some(row) => Response::from_json(&list_document(&row)),
        None => Response::error("list not found", 404),
    }
}

pub(crate) async fn delete_list_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let list_id = list_id_from_context(&ctx)?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if !delete_list_row(&db, &account.id, &list_id).await? {
        return Response::error("list not found", 404);
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn list_accounts_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let list_id = list_id_from_context(&ctx)?;
    if list_row_by_id(&db, &account.id, &list_id).await?.is_none() {
        return Response::error("list not found", 404);
    }

    let max_id = req
        .url()?
        .query_pairs()
        .find(|(key, _)| key == "max_id")
        .map(|(_, value)| value.into_owned());
    let mut documents = Vec::new();
    for row in list_membership_refs(&db, &list_id).await? {
        if let Some(document) =
            resolve_list_member_document(&db, &config, &row.target_account_ref).await?
        {
            documents.push(document);
        }
    }
    if let Some(max_id) = max_id {
        documents.retain(|document| {
            document
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(|value| value < max_id.as_str())
                .unwrap_or(false)
        });
    }
    Response::from_json(&documents)
}

pub(crate) async fn add_list_accounts_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let list_id = list_id_from_context(&ctx)?;
    let request = parse_list_accounts_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if list_row_by_id(&db, &account.id, &list_id).await?.is_none() {
        return Response::error("list not found", 404);
    }

    add_accounts_to_list(&db, &list_id, &request.account_ids.unwrap_or_default()).await?;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn delete_list_accounts_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let list_id = list_id_from_context(&ctx)?;
    let request = parse_list_accounts_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if list_row_by_id(&db, &account.id, &list_id).await?.is_none() {
        return Response::error("list not found", 404);
    }

    for account_ref in request.account_ids.unwrap_or_default() {
        let Some(variants) =
            requested_account_membership_variants(&db, &config, &account_ref).await?
        else {
            continue;
        };
        remove_accounts_from_list(&db, &list_id, &variants).await?;
    }

    Response::from_json(&serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::{LocalAccount, ProfileField};

    fn fixture_account() -> LocalAccount {
        LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: vec![ProfileField {
                name: "website".to_owned(),
                value: "https://example.com".to_owned(),
            }],
            locked: false,
            bot: false,
            discoverable: true,
            default_post_visibility: "public".to_owned(),
            default_quote_policy: "public".to_owned(),
            default_sensitive: false,
            default_language: Some("en".to_owned()),
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn normalize_replies_policy_accepts_supported_values() {
        assert_eq!(normalize_replies_policy(None).unwrap(), "list");
        assert_eq!(
            normalize_replies_policy(Some("followed")).unwrap(),
            "followed"
        );
        assert_eq!(normalize_replies_policy(Some("LIST")).unwrap(), "list");
        assert_eq!(normalize_replies_policy(Some(" none ")).unwrap(), "none");
    }

    #[test]
    fn normalize_replies_policy_rejects_unknown_value() {
        assert!(normalize_replies_policy(Some("all")).is_err());
    }

    #[test]
    fn parse_form_bool_accepts_expected_variants() {
        assert_eq!(parse_form_bool(Some("true".to_owned())), Some(true));
        assert_eq!(parse_form_bool(Some("1".to_owned())), Some(true));
        assert_eq!(parse_form_bool(Some("on".to_owned())), Some(true));
        assert_eq!(parse_form_bool(Some("false".to_owned())), Some(false));
        assert_eq!(parse_form_bool(Some("0".to_owned())), Some(false));
        assert_eq!(parse_form_bool(Some("off".to_owned())), Some(false));
        assert_eq!(parse_form_bool(Some("maybe".to_owned())), None);
    }

    #[test]
    fn list_document_includes_exclusive_flag() {
        let row = AccountListRow {
            id: "list-1".to_owned(),
            title: "Friends".to_owned(),
            replies_policy: "list".to_owned(),
            exclusive: 1,
        };

        let document = list_document(&row);
        assert_eq!(document["exclusive"], serde_json::json!(true));
    }

    #[test]
    fn list_membership_variants_cover_local_id_and_address() {
        let config = cfwdon_core::AppConfig::new("social.example", "cfwdon", "test");
        let account = fixture_account();
        assert_eq!(
            list_membership_variants_for_local_account(&account, &config),
            ["acct-1".to_owned(), "alice@social.example".to_owned()]
        );
    }

    #[test]
    fn list_membership_variants_cover_remote_actor_uri_and_address() {
        let actor = crate::RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: "2026-01-02 03:04:05".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: String::new(),
            summary_html: String::new(),
            profile_url: None,
            avatar_url: None,
            header_url: None,
        };
        assert_eq!(
            list_membership_variants_for_remote_actor(&actor),
            [
                "https://remote.example/users/alice".to_owned(),
                "alice@remote.example".to_owned()
            ]
        );
    }
}
