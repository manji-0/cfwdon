use crate::{
    Request, Response, Result, RouteContext, build_internal_cursor_link_for_url,
    build_local_status_response, delete_conversation_for_account, extract_authenticated_user,
    find_account_by_id, find_conversation_for_account, find_media_attachments_by_status_id,
    find_remote_actor_by_actor_uri, find_remote_actor_by_username_domain, find_status_by_id,
    list_conversation_participants, list_conversations_for_account, load_account_stats,
    load_config, mark_conversation_read, mark_conversation_unread, parse_lookup_handle,
    resolve_local_account,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct ConversationsQuery {
    limit: Option<u32>,
    max_id: Option<String>,
    since_id: Option<String>,
    min_id: Option<String>,
}

fn conversations_limit(value: Option<u32>) -> u32 {
    value.unwrap_or(20).clamp(1, 40)
}

async fn participant_account_documents(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    conversation_id: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut accounts = Vec::new();
    let refs = list_conversation_participants(db, conversation_id).await?;
    for participant_ref in refs {
        if participant_ref == owner.id {
            continue;
        }
        if let Some(account) = find_account_by_id(db, &participant_ref).await? {
            let stats = load_account_stats(db, &account.id).await?;
            accounts.push(serde_json::to_value(
                crate::MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
            )?);
            continue;
        }
        if let Some(actor) = find_remote_actor_by_actor_uri(db, &participant_ref).await? {
            accounts.push(serde_json::to_value(
                crate::MastodonAccountResponse::from_remote_actor(&actor),
            )?);
            continue;
        }
        if participant_ref.contains('@') {
            let handle = parse_lookup_handle(&participant_ref, config).map_err(|error| {
                worker::Error::RustError(format!("invalid conversation participant ref: {error}"))
            })?;
            if let Some(domain) = handle.domain.as_deref()
                && domain != config.instance_domain
                && let Some(actor) =
                    find_remote_actor_by_username_domain(db, &handle.username, domain).await?
            {
                accounts.push(serde_json::to_value(
                    crate::MastodonAccountResponse::from_remote_actor(&actor),
                )?);
            }
        }
    }

    if accounts.is_empty() {
        let stats = load_account_stats(db, &owner.id).await?;
        accounts.push(serde_json::to_value(
            crate::MastodonAccountResponse::from_account_with_stats(owner, config, &stats),
        )?);
    }
    Ok(accounts)
}

async fn last_status_document(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    last_status_id: Option<&str>,
) -> Result<Option<serde_json::Value>> {
    let Some(last_status_id) = last_status_id else {
        return Ok(None);
    };
    let Some(status) = find_status_by_id(db, last_status_id).await? else {
        return Ok(None);
    };
    let Some(author) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(None);
    };
    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    let in_reply_to_account_id = crate::load_in_reply_to_account_id(db, &status).await?;
    Ok(Some(serde_json::to_value(
        build_local_status_response(
            db,
            config,
            Some(owner),
            &status,
            &author,
            in_reply_to_account_id,
            media,
        )
        .await?,
    )?))
}

async fn conversation_document(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &crate::ConversationRow,
) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": row.id,
        "unread": row.unread != 0,
        "accounts": participant_account_documents(db, config, owner, &row.id).await?,
        "last_status": last_status_document(db, config, owner, row.last_status_id.as_deref()).await?,
    }))
}

fn conversations_link_header(
    req: &Request,
    limit: u32,
    first_id: Option<&str>,
    last_id: Option<&str>,
) -> Result<Option<String>> {
    let url = req.url()?;
    let mut links = Vec::new();
    if let Some(last_id) = last_id.filter(|value| !value.is_empty()) {
        let mut next = build_internal_cursor_link_for_url(&url, limit, Some(0), None, "next")?;
        next = next.replace("max_id=0", &format!("max_id={last_id}"));
        links.push(next);
    }
    if let Some(first_id) = first_id.filter(|value| !value.is_empty()) {
        let mut prev = build_internal_cursor_link_for_url(&url, limit, None, Some(0), "prev")?;
        prev = prev.replace("since_id=0", &format!("min_id={first_id}"));
        links.push(prev.replace("since_id", "min_id"));
    }
    Ok((!links.is_empty()).then(|| links.join(", ")))
}

pub(crate) async fn conversations_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: ConversationsQuery = req.query().unwrap_or_default();
    let db = ctx.d1(&config.database_binding)?;
    let owner = resolve_local_account(&db, &user).await?;
    let rows = list_conversations_for_account(
        &db,
        &owner.id,
        conversations_limit(query.limit),
        query.max_id.as_deref(),
        query.min_id.as_deref().or(query.since_id.as_deref()),
    )
    .await?;

    let mut documents = Vec::with_capacity(rows.len());
    for row in &rows {
        documents.push(conversation_document(&db, &config, &owner, row).await?);
    }

    let first_id = documents
        .first()
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str);
    let last_id = documents
        .last()
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str);
    let mut builder = Response::from_json(&documents)?;
    if let Some(link) =
        conversations_link_header(&req, conversations_limit(query.limit), first_id, last_id)?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}

pub(crate) async fn delete_conversation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let conversation_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing conversation id route parameter".to_owned())
        })?;
    let db = ctx.d1(&config.database_binding)?;
    let owner = resolve_local_account(&db, &user).await?;
    if !delete_conversation_for_account(&db, &owner.id, &conversation_id).await? {
        return Response::error("conversation not found", 404);
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn read_conversation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let conversation_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing conversation id route parameter".to_owned())
        })?;
    let db = ctx.d1(&config.database_binding)?;
    let owner = resolve_local_account(&db, &user).await?;
    if !mark_conversation_read(&db, &owner.id, &conversation_id).await? {
        return Response::error("conversation not found", 404);
    }
    let Some(row) = find_conversation_for_account(&db, &owner.id, &conversation_id).await? else {
        return Response::error("conversation not found", 404);
    };
    Response::from_json(&conversation_document(&db, &config, &owner, &row).await?)
}

pub(crate) async fn unread_conversation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let conversation_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing conversation id route parameter".to_owned())
        })?;
    let db = ctx.d1(&config.database_binding)?;
    let owner = resolve_local_account(&db, &user).await?;
    if !mark_conversation_unread(&db, &owner.id, &conversation_id).await? {
        return Response::error("conversation not found", 404);
    }
    let Some(row) = find_conversation_for_account(&db, &owner.id, &conversation_id).await? else {
        return Response::error("conversation not found", 404);
    };
    Response::from_json(&conversation_document(&db, &config, &owner, &row).await?)
}
