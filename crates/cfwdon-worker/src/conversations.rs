use crate::accounts::{
    AccountStats, find_accounts_by_ids, load_account_stats, load_account_stats_map,
};
use crate::auth::{find_account_by_id, find_authenticated_local_account};
use crate::conversation_store::{
    ConversationRow, delete_conversation_for_account, find_conversation_for_account,
    list_conversation_participants, list_conversation_participants_by_ids,
    list_conversations_for_account, mark_conversation_read, mark_conversation_unread,
};
use crate::db_session::bind_request_d1;
use crate::identity::parse_lookup_handle;
use crate::media::find_media_attachments_by_status_id;
use crate::request_utils::build_internal_cursor_link_for_url;
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use crate::statuses::{
    build_local_status_response, find_status_by_id, find_statuses_by_ids,
    load_in_reply_to_account_id,
};
use crate::store::remote::{
    RemoteActorRow, find_remote_actor_by_actor_uri, find_remote_actor_by_username_domain,
    find_remote_actors_by_actor_uris,
};
use crate::timelines::{StatusRenderItem, render_status_items};
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use std::collections::HashMap;
use worker::{Request, Response, Result, RouteContext};

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

pub(crate) async fn participant_account_documents(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    conversation_id: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut accounts = Vec::new();
    let refs = list_conversation_participants(db, conversation_id).await?;
    for participant_ref in refs {
        if participant_ref == owner.id() {
            continue;
        }
        if let Some(account) = find_account_by_id(db, &participant_ref).await? {
            let stats = load_account_stats(db, account.id()).await?;
            accounts.push(serde_json::to_value(
                MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
            )?);
            continue;
        }
        if let Some(actor) = find_remote_actor_by_actor_uri(db, &participant_ref).await? {
            accounts.push(serde_json::to_value(
                MastodonAccountResponse::from_remote_actor(&actor),
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
                    MastodonAccountResponse::from_remote_actor(&actor),
                )?);
            }
        }
    }

    if accounts.is_empty() {
        let stats = load_account_stats(db, owner.id()).await?;
        accounts.push(serde_json::to_value(
            MastodonAccountResponse::from_account_with_stats(owner, config, &stats),
        )?);
    }
    Ok(accounts)
}

pub(crate) async fn last_status_document(
    db: &D1Database,
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
    let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
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

pub(crate) async fn conversation_document(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &ConversationRow,
) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": row.id,
        "unread": row.unread != 0,
        "accounts": participant_account_documents(db, config, owner, &row.id).await?,
        "last_status": last_status_document(db, config, owner, row.last_status_id.as_deref()).await?,
    }))
}

/// Render a page of conversations with batched participant, account, stats,
/// and last-status lookups; [`conversation_document`] renders one.
async fn conversation_documents(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    rows: &[ConversationRow],
) -> Result<Vec<serde_json::Value>> {
    let conversation_ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let last_status_ids = rows
        .iter()
        .filter_map(|row| row.last_status_id.clone())
        .collect::<Vec<_>>();
    let (participants_by_conversation, last_statuses) = futures_util::try_join!(
        list_conversation_participants_by_ids(db, &conversation_ids),
        find_statuses_by_ids(db, &last_status_ids),
    )?;

    let participant_refs = participants_by_conversation
        .values()
        .flatten()
        .filter(|participant_ref| participant_ref.as_str() != owner.id())
        .cloned()
        .collect::<Vec<_>>();
    let mut stats_account_ids = participant_refs.clone();
    stats_account_ids.push(owner.id().to_owned());
    let mut last_status_order = HashMap::new();
    let mut render_items = Vec::with_capacity(last_statuses.len());
    for (index, status) in last_statuses.into_iter().enumerate() {
        last_status_order.insert(status.id.clone(), index);
        render_items.push(StatusRenderItem::Local(status));
    }
    let (accounts_by_id, actors_by_uri, stats_by_account_id, rendered_statuses) = futures_util::try_join!(
        find_accounts_by_ids(db, &participant_refs),
        find_remote_actors_by_actor_uris(db, &participant_refs),
        load_account_stats_map(db, &stats_account_ids),
        render_status_items(db, config, Some(owner), render_items),
    )?;

    let no_stats = AccountStats::default();
    let mut documents = Vec::with_capacity(rows.len());
    for row in rows {
        let mut accounts = Vec::new();
        for participant_ref in participants_by_conversation
            .get(&row.id)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if participant_ref == owner.id() {
                continue;
            }
            if let Some(account) = accounts_by_id.get(participant_ref) {
                let stats = stats_by_account_id.get(account.id()).unwrap_or(&no_stats);
                accounts.push(serde_json::to_value(
                    MastodonAccountResponse::from_account_with_stats(account, config, stats),
                )?);
            } else if let Some(actor) = actors_by_uri.get(participant_ref) {
                accounts.push(serde_json::to_value(
                    MastodonAccountResponse::from_remote_actor(actor),
                )?);
            } else if let Some(actor) =
                find_remote_participant_by_handle(db, config, participant_ref).await?
            {
                accounts.push(serde_json::to_value(
                    MastodonAccountResponse::from_remote_actor(&actor),
                )?);
            }
        }
        if accounts.is_empty() {
            let stats = stats_by_account_id.get(owner.id()).unwrap_or(&no_stats);
            accounts.push(serde_json::to_value(
                MastodonAccountResponse::from_account_with_stats(owner, config, stats),
            )?);
        }
        let last_status = row
            .last_status_id
            .as_ref()
            .and_then(|id| last_status_order.get(id))
            .and_then(|index| rendered_statuses.get(*index))
            .cloned();
        documents.push(serde_json::json!({
            "id": row.id,
            "unread": row.unread != 0,
            "accounts": accounts,
            "last_status": last_status,
        }));
    }
    Ok(documents)
}

/// `user@domain` participant refs that are not stored as actor URIs.
async fn find_remote_participant_by_handle(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    participant_ref: &str,
) -> Result<Option<RemoteActorRow>> {
    if !participant_ref.contains('@') {
        return Ok(None);
    }
    let handle = parse_lookup_handle(participant_ref, config).map_err(|error| {
        worker::Error::RustError(format!("invalid conversation participant ref: {error}"))
    })?;
    match handle.domain.as_deref() {
        Some(domain) if domain != config.instance_domain => {
            find_remote_actor_by_username_domain(db, &handle.username, domain).await
        }
        _ => Ok(None),
    }
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
    let query: ConversationsQuery = req.query().unwrap_or_default();
    let db = bind_request_d1(&ctx, &config)?;
    let owner = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(owner) => owner,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let rows = list_conversations_for_account(
        &db,
        owner.id(),
        conversations_limit(query.limit),
        query.max_id.as_deref(),
        query.min_id.as_deref().or(query.since_id.as_deref()),
    )
    .await?;

    let documents = conversation_documents(&db, &config, &owner, &rows).await?;

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
    let conversation_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing conversation id route parameter".to_owned())
        })?;
    let db = bind_request_d1(&ctx, &config)?;
    let owner = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(owner) => owner,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if !delete_conversation_for_account(&db, owner.id(), &conversation_id).await? {
        return Response::error("conversation not found", 404);
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn read_conversation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let conversation_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing conversation id route parameter".to_owned())
        })?;
    let db = bind_request_d1(&ctx, &config)?;
    let owner = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(owner) => owner,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if !mark_conversation_read(&db, owner.id(), &conversation_id).await? {
        return Response::error("conversation not found", 404);
    }
    let Some(row) = find_conversation_for_account(&db, owner.id(), &conversation_id).await? else {
        return Response::error("conversation not found", 404);
    };
    Response::from_json(&conversation_document(&db, &config, &owner, &row).await?)
}

pub(crate) async fn unread_conversation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let conversation_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing conversation id route parameter".to_owned())
        })?;
    let db = bind_request_d1(&ctx, &config)?;
    let owner = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(owner) => owner,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if !mark_conversation_unread(&db, owner.id(), &conversation_id).await? {
        return Response::error("conversation not found", 404);
    }
    let Some(row) = find_conversation_for_account(&db, owner.id(), &conversation_id).await? else {
        return Response::error("conversation not found", 404);
    };
    Response::from_json(&conversation_document(&db, &config, &owner, &row).await?)
}
