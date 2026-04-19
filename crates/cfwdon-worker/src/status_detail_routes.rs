use super::{
    Error, MastodonAccountResponse, Request, Response, Result, RouteContext,
    build_activitypub_note, build_local_status_context, build_local_status_response,
    build_remote_status_context, build_remote_status_response, can_view_local_status,
    find_account_by_id, find_account_by_username, find_authenticated_local_account,
    find_local_status_by_object_uri, find_media_attachments_by_status_id,
    find_remote_actor_by_actor_uri, find_remote_status_by_id,
    find_remote_status_by_url_or_object_uri, find_status_by_id, is_public_activitypub_visibility,
    list_local_favourite_account_ids_for_remote_status,
    list_local_favourite_account_ids_for_status, list_local_reblog_account_ids_for_remote_status,
    list_local_reblog_account_ids_for_status, list_remote_favourite_actor_uris_for_status,
    list_remote_reblog_actor_uris_for_status, list_remote_status_edit_snapshots,
    load_account_stats, load_config, load_in_reply_to_account_id, load_remote_status_updated_at,
    remote_account_rest_id, status_id_from_context, strip_html_tags,
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Default, Deserialize)]
struct StatusInteractionAccountsQuery {
    limit: Option<u32>,
}

pub(crate) enum ResolvedStatus {
    Local(crate::StatusRow),
    Remote(crate::RemoteStatusRow),
}

#[derive(Debug, Serialize)]
struct StatusSourceResponse {
    id: String,
    text: String,
    spoiler_text: String,
}

pub(crate) fn normalize_status_history_entry(mut value: serde_json::Value) -> serde_json::Value {
    let content = value
        .get("content")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(""));
    let spoiler_text = value
        .get("spoiler_text")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(""));
    let sensitive = value
        .get("sensitive")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(false));
    let created_at = value
        .get("created_at")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(""));
    let account = value
        .get("account")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let media_attachments = match value.get_mut("media_attachments") {
        Some(serde_json::Value::Array(items)) => serde_json::Value::Array(items.clone()),
        _ => serde_json::json!([]),
    };
    let emojis = match value.get_mut("emojis") {
        Some(serde_json::Value::Array(items)) => serde_json::Value::Array(items.clone()),
        _ => serde_json::json!([]),
    };
    let poll = value
        .get("poll")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let quote = value
        .get("quote")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    serde_json::json!({
        "content": content,
        "spoiler_text": spoiler_text,
        "sensitive": sensitive,
        "created_at": created_at,
        "account": account,
        "media_attachments": media_attachments,
        "emojis": emojis,
        "poll": poll,
        "quote": quote,
    })
}

pub(crate) fn first_url_from_text(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let trimmed = token
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.'
                )
            })
            .trim();
        (!trimmed.is_empty()
            && (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
            && Url::parse(trimmed).is_ok())
        .then(|| trimmed.to_owned())
    })
}

pub(crate) fn build_status_card_value(text: &str) -> Option<serde_json::Value> {
    let url = first_url_from_text(text)?;
    let parsed = Url::parse(&url).ok()?;
    let provider_name = parsed.host_str().unwrap_or_default().to_owned();
    Some(serde_json::json!({
        "url": url,
        "title": provider_name,
        "description": "",
        "type": "link",
        "author_name": "",
        "author_url": "",
        "provider_name": provider_name,
        "provider_url": format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or_default()),
        "html": "",
        "width": 0,
        "height": 0,
        "image": serde_json::Value::Null,
        "embed_url": "",
        "blurhash": serde_json::Value::Null,
    }))
}

pub(crate) async fn resolve_status_reference(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    id: &str,
) -> Result<Option<ResolvedStatus>> {
    let raw_id = id.trim();
    if raw_id.is_empty() {
        return Ok(None);
    }

    if let Some(status) = find_status_by_id(db, raw_id).await? {
        return Ok(Some(ResolvedStatus::Local(status)));
    }
    if let Some(status) = find_remote_status_by_id(db, raw_id).await? {
        return Ok(Some(ResolvedStatus::Remote(status)));
    }

    let decoded_id = urlencoding::decode(raw_id)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| raw_id.to_owned());
    if let Some(status) = find_local_status_by_object_uri(db, config, &decoded_id).await? {
        return Ok(Some(ResolvedStatus::Local(status)));
    }
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, &decoded_id).await? {
        return Ok(Some(ResolvedStatus::Remote(status)));
    }

    Ok(None)
}

async fn build_local_interaction_account_responses(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ids: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let mut responses = Vec::new();

    for account_id in account_ids {
        let Some(account) = find_account_by_id(db, account_id).await? else {
            continue;
        };
        let stats = load_account_stats(db, &account.id).await?;
        responses.push(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        ));
    }

    Ok(responses)
}

async fn build_remote_interaction_account_response(
    db: &worker::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    let status_summary = crate::load_remote_actor_status_summary(db, actor_uri).await?;

    if let Some(actor) = find_remote_actor_by_actor_uri(db, actor_uri).await? {
        let mut response = MastodonAccountResponse::from_remote_actor(&actor);
        response.statuses_count = status_summary.statuses_count;
        response.last_status_at = status_summary.last_status_at.clone();
        return Ok(Some(response));
    }

    let parsed = match Url::parse(actor_uri) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(None),
    };
    let Some(domain) = parsed.host_str().map(str::to_owned) else {
        return Ok(None);
    };
    let Some(username) = parsed
        .path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
        .map(|segment| segment.trim_start_matches('@').to_owned())
        .filter(|segment| !segment.is_empty())
    else {
        return Ok(None);
    };

    Ok(Some(MastodonAccountResponse {
        id: remote_account_rest_id(actor_uri),
        username: username.clone(),
        acct: format!("{username}@{domain}"),
        uri: actor_uri.to_owned(),
        display_name: username.clone(),
        locked: false,
        bot: false,
        group: false,
        discoverable: true,
        indexable: true,
        noindex: None,
        hide_collections: None,
        show_media: None,
        show_media_replies: None,
        show_featured: None,
        last_status_at: status_summary.last_status_at,
        created_at: String::new(),
        note: String::new(),
        url: actor_uri.to_owned(),
        avatar: String::new(),
        avatar_static: String::new(),
        header: String::new(),
        header_static: String::new(),
        emojis: Vec::new(),
        fields: Vec::new(),
        roles: Vec::new(),
        followers_count: 0,
        following_count: 0,
        statuses_count: status_summary.statuses_count,
        source: None,
    }))
}

async fn build_remote_interaction_account_responses(
    db: &worker::D1Database,
    actor_uris: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let mut responses = Vec::new();

    for actor_uri in actor_uris {
        if let Some(response) = build_remote_interaction_account_response(db, actor_uri).await? {
            responses.push(response);
        }
    }

    Ok(responses)
}

pub(crate) async fn status_reblogged_by_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let query: StatusInteractionAccountsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).min(80);

    let db = ctx.d1(&config.database_binding)?;
    let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
        return Response::error("status not found", 404);
    };

    let mut responses = match status {
        ResolvedStatus::Local(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            let local_accounts =
                list_local_reblog_account_ids_for_status(&db, &status.id, limit).await?;
            let mut responses =
                build_local_interaction_account_responses(&db, &config, &local_accounts).await?;
            if responses.len() < limit as usize {
                let remaining = limit.saturating_sub(responses.len() as u32);
                let remote_actor_uris =
                    list_remote_reblog_actor_uris_for_status(&db, &status.id, remaining).await?;
                responses.extend(
                    build_remote_interaction_account_responses(&db, &remote_actor_uris).await?,
                );
            }
            responses
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            let local_accounts =
                list_local_reblog_account_ids_for_remote_status(&db, &status.id, limit).await?;
            build_local_interaction_account_responses(&db, &config, &local_accounts).await?
        }
    };
    responses.truncate(limit as usize);
    Response::from_json(&responses)
}

pub(crate) async fn status_favourited_by_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let query: StatusInteractionAccountsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).min(80);

    let db = ctx.d1(&config.database_binding)?;
    let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
        return Response::error("status not found", 404);
    };

    let mut responses = match status {
        ResolvedStatus::Local(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            let local_accounts =
                list_local_favourite_account_ids_for_status(&db, &status.id, limit).await?;
            let mut responses =
                build_local_interaction_account_responses(&db, &config, &local_accounts).await?;
            if responses.len() < limit as usize {
                let remaining = limit.saturating_sub(responses.len() as u32);
                let remote_actor_uris =
                    list_remote_favourite_actor_uris_for_status(&db, &status.id, remaining).await?;
                responses.extend(
                    build_remote_interaction_account_responses(&db, &remote_actor_uris).await?,
                );
            }
            responses
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            let local_accounts =
                list_local_favourite_account_ids_for_remote_status(&db, &status.id, limit).await?;
            build_local_interaction_account_responses(&db, &config, &local_accounts).await?
        }
    };
    responses.truncate(limit as usize);
    Response::from_json(&responses)
}

pub(crate) async fn status_object_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing status id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != account.id {
        return Response::error("status not found", 404);
    }
    if !is_public_activitypub_visibility(&status.visibility) {
        return Response::error("status not found", 404);
    }

    let note = build_activitypub_note(&db, &config, &account, &status, true).await?;
    super::json_response(&note, "application/activity+json", &[])
}

pub(crate) async fn status_card_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _ = req;
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
        return Response::error("status not found", 404);
    };

    let card = match status {
        ResolvedStatus::Local(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            build_status_card_value(&status._text_content)
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            build_status_card_value(&strip_html_tags(&status.content_html))
        }
    }
    .unwrap_or(serde_json::Value::Null);

    Response::from_json(&card)
}

pub(crate) async fn status_api_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
        return Response::error("status not found", 404);
    };

    match status {
        ResolvedStatus::Local(status) => {
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                return Response::error("status not found", 404);
            };
            if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
                return Response::error("status not found", 404);
            }

            let media = find_media_attachments_by_status_id(&db, &status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
            Response::from_json(
                &build_local_status_response(
                    &db,
                    &config,
                    viewer.as_ref(),
                    &status,
                    &account,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            )
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
                return Response::error("status not found", 404);
            };
            Response::from_json(
                &crate::build_remote_status_response(
                    &db,
                    &config,
                    viewer.as_ref(),
                    &status,
                    &actor,
                )
                .await?,
            )
        }
    }
}

pub(crate) async fn status_source_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let Some(viewer) = find_authenticated_local_account(&req, &db, &config).await? else {
        return Response::error("Cloudflare Access authentication required", 401);
    };
    let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    let ResolvedStatus::Local(status) = status else {
        return Response::error("status source is only available for local statuses", 403);
    };
    let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
        return Response::error("status not found", 404);
    };
    if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
        return Response::error("status not found", 404);
    }

    Response::from_json(&StatusSourceResponse {
        id: status.id,
        text: status._text_content,
        spoiler_text: status.spoiler_text,
    })
}

pub(crate) async fn status_context_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;

    let Some(status) = resolve_status_reference(&db, &config, &status_id).await? else {
        return Response::error("status not found", 404);
    };

    match status {
        ResolvedStatus::Local(status) => {
            let Some(owner) = find_account_by_id(&db, &status.account_id).await? else {
                return Response::error("status not found", 404);
            };
            if !can_view_local_status(&db, &status, viewer.as_ref(), &owner).await? {
                return Response::error("status not found", 404);
            }

            Response::from_json(
                &build_local_status_context(&db, &config, viewer.as_ref(), &status, &owner).await?,
            )
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
                return Response::error("status not found", 404);
            };
            Response::from_json(&build_remote_status_context(&db, &config, &status, &actor).await?)
        }
    }
}

pub(crate) async fn status_history_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
            return Response::error("status not found", 404);
        }

        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
        let response = build_local_status_response(
            &db,
            &config,
            viewer.as_ref(),
            &status,
            &account,
            in_reply_to_account_id,
            media,
        )
        .await?;
        let mut current_revision = serde_json::to_value(response).unwrap_or(serde_json::json!({}));
        let created_at = crate::load_status_updated_at(&db, &status.id)
            .await?
            .unwrap_or_else(|| status.created_at.clone());
        current_revision["created_at"] = serde_json::json!(created_at);
        let mut history = vec![normalize_status_history_entry(current_revision)];
        history.extend(crate::list_status_edit_snapshots(&db, &status.id).await?);
        return Response::from_json(&history);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };

        let response =
            build_remote_status_response(&db, &config, viewer.as_ref(), &status, &actor).await?;
        let mut current_revision = serde_json::to_value(response).unwrap_or(serde_json::json!({}));
        let created_at = load_remote_status_updated_at(&db, &status.id)
            .await?
            .unwrap_or_else(|| status.published_at.clone());
        current_revision["created_at"] = serde_json::json!(created_at);
        let mut history = vec![normalize_status_history_entry(current_revision)];
        history.extend(list_remote_status_edit_snapshots(&db, &status.id).await?);
        return Response::from_json(&history);
    }

    Response::error("status not found", 404)
}
