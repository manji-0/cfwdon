use super::{
    DeleteStatusQuery, MastodonStatusResponse, Request, Response, Result, RouteContext, StatusRow,
    UpdateMediaRequest, UpdateStatusRequest, actor_url, app_bearer_token_from_request,
    attach_media_to_status, build_local_status_response, can_view_local_status,
    delete_media_attachments, delete_status_by_id, effective_local_quote_approval_policy,
    enqueue_outbox_activity, enqueue_outbox_delete, enqueue_status_update_activity,
    ensure_direct_conversation_for_status, extract_authenticated_user, extract_mentions_from_text,
    find_account_by_id, find_account_by_username,
    find_authenticated_local_account, find_local_status_by_object_uri,
    find_media_attachments_by_status_id, find_oauth_access_token_by_bearer_token,
    find_oauth_app_by_bearer_token, find_remote_status_by_id,
    find_remote_status_by_url_or_object_uri, find_status_by_id, find_status_poll_by_status_id,
    insert_status, insert_status_edit_snapshot, is_blocking_actor, is_local_follower_authorized,
    list_status_poll_options, load_config, load_in_reply_to_account_id,
    load_mastodon_poll_response, normalize_status_history_entry, normalize_status_poll,
    now_iso_string, oauth_access_token_has_any_scope, parse_status_draft,
    parse_update_status_request, replace_status_media, replace_status_poll,
    resolve_attachable_media, resolve_editable_media, resolve_local_account,
    send_push_notification, send_status_quote_notification, send_status_update_notifications,
    status_id_from_context, update_local_status,
    validate_scheduled_at_minimum_offset,
};
use cfwdon_domain::{LocalAccount, StatusDraft, Visibility};

struct CreateStatusAccess {
    account: LocalAccount,
    application_id: Option<i64>,
}

pub(crate) fn initial_local_quote_approval_policy<'a>(
    account: &'a LocalAccount,
    draft: &'a StatusDraft,
) -> &'a str {
    if matches!(
        draft.visibility,
        Visibility::FollowersOnly | Visibility::Direct
    ) {
        "nobody"
    } else {
        draft
            .quote_approval_policy
            .as_deref()
            .unwrap_or(account.default_quote_policy.as_str())
    }
}

pub(crate) fn local_quote_policy_allows(policy: &str, is_owner: bool, is_follower: bool) -> bool {
    if is_owner {
        return true;
    }

    match policy {
        "public" => true,
        "followers" => is_follower,
        _ => false,
    }
}

pub(crate) fn remote_quote_state_for_local_target(
    status: &StatusRow,
    remote_actor_follows_owner: bool,
    blocked_by_owner: bool,
) -> &'static str {
    if blocked_by_owner {
        return "rejected";
    }
    if local_quote_policy_allows(
        effective_local_quote_approval_policy(status),
        false,
        remote_actor_follows_owner,
    ) {
        "accepted"
    } else {
        "pending"
    }
}

pub(crate) async fn initial_local_quote_state(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    quote_of_uri: Option<&str>,
) -> Result<&'static str> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok("accepted");
    };

    if find_local_status_by_object_uri(db, config, quote_of_uri)
        .await?
        .is_some()
    {
        Ok("accepted")
    } else {
        Ok("pending")
    }
}

async fn validate_local_quote_creation(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &LocalAccount,
    draft: &StatusDraft,
    quote_of_uri: &str,
) -> Result<Option<&'static str>> {
    let Some(status) = find_local_status_by_object_uri(db, config, quote_of_uri).await? else {
        return Ok(None);
    };
    let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(Some("quoted_status_id references unknown status"));
    };
    if !can_view_local_status(db, &status, Some(requester), &owner).await? {
        return Ok(Some("quoted_status_id references unknown status"));
    }
    if status.visibility == "direct" {
        return Ok(Some("private mentions cannot be quoted"));
    }
    if status.visibility == "private" && draft.visibility.as_str() != "private" {
        return Ok(Some("private posts can only be quoted in private posts"));
    }
    let owner_actor_uri = actor_url(config, &owner.username);
    let requester_actor_uri = actor_url(config, &requester.username);
    if is_blocking_actor(db, &owner.id, &requester_actor_uri).await?
        || is_blocking_actor(db, &requester.id, &owner_actor_uri).await?
    {
        return Ok(Some("current user is not allowed to quote this status"));
    }

    let is_owner = requester.id == owner.id;
    let is_follower = if is_owner {
        false
    } else {
        is_local_follower_authorized(db, &requester.id, &owner.id).await?
    };
    if !local_quote_policy_allows(
        effective_local_quote_approval_policy(&status),
        is_owner,
        is_follower,
    ) {
        return Ok(Some("current user is not allowed to quote this status"));
    }

    Ok(None)
}

async fn resolve_quoted_status_uri(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    value: Option<&str>,
) -> Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if let Some(status) = find_status_by_id(db, value).await? {
        return Ok(Some(
            status
                .ap_id
                .clone()
                .unwrap_or_else(|| format!("local:{}", status.id)),
        ));
    }
    if let Some(status) = find_remote_status_by_id(db, value).await? {
        return Ok(Some(status.object_uri));
    }
    if let Some(status) = find_local_status_by_object_uri(db, config, value).await? {
        return Ok(Some(
            status
                .ap_id
                .clone()
                .unwrap_or_else(|| format!("local:{}", status.id)),
        ));
    }
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, value).await? {
        return Ok(Some(status.object_uri));
    }

    Ok(None)
}

async fn resolve_create_status_access(
    req: &Request,
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<Option<CreateStatusAccess>> {
    if let Some(token) = app_bearer_token_from_request(req)? {
        if let Some(access_token) = find_oauth_access_token_by_bearer_token(db, &token).await? {
            if !oauth_access_token_has_any_scope(&access_token, &["write:statuses", "write"]) {
                return Err(worker::Error::RustError(
                    "status token outside authorized scopes".to_owned(),
                ));
            }
            let Some(account) = find_account_by_id(db, &access_token.account_id).await? else {
                return Ok(None);
            };
            return Ok(Some(CreateStatusAccess {
                account,
                application_id: Some(access_token.oauth_app_id),
            }));
        }
        if let Some(app) = find_oauth_app_by_bearer_token(db, &token).await? {
            let Some(user) = extract_authenticated_user(req, config).await? else {
                return Ok(None);
            };
            let account = resolve_local_account(db, &user).await?;
            return Ok(Some(CreateStatusAccess {
                account,
                application_id: Some(app.id),
            }));
        }
        return Ok(None);
    }

    let Some(user) = extract_authenticated_user(req, config).await? else {
        return Ok(None);
    };
    Ok(Some(CreateStatusAccess {
        account: resolve_local_account(db, &user).await?,
        application_id: None,
    }))
}

pub(crate) async fn create_status(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let parsed = match parse_status_draft(&mut req).await {
        Ok(draft) => draft,
        Err(message) => return Response::error(message, 422),
    };
    let super::ParsedStatusDraft {
        draft,
        idempotency_key,
        scheduled_at,
        quoted_status_id,
    } = parsed;
    let db = ctx.d1(&config.database_binding)?;
    let access = match resolve_create_status_access(&req, &db, &config).await {
        Ok(Some(access)) => access,
        Ok(None) => {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "The access token is invalid",
            }))?
            .with_status(401));
        }
        Err(worker::Error::RustError(message))
            if message == "status token outside authorized scopes" =>
        {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "This action is outside the authorized scopes",
            }))?
            .with_status(403));
        }
        Err(error) => return Err(error),
    };
    let pending_media = match resolve_attachable_media(&db, &access.account, &draft.media_ids).await
    {
        Ok(media) => media,
        Err(message) => return Response::error(message, 422),
    };
    let in_reply_to_account_id = match draft.in_reply_to_id.as_deref() {
        Some(status_id) => match find_status_by_id(&db, status_id).await? {
            Some(status) => Some(status.account_id),
            None => return Response::error("in_reply_to_id references unknown local status", 422),
        },
        None => None,
    };
    let quote_of_uri =
        match resolve_quoted_status_uri(&db, &config, quoted_status_id.as_deref()).await? {
            Some(uri) => Some(uri),
            None if quoted_status_id.is_some() => {
                return Response::error("quoted_status_id references unknown status", 422);
            }
            None => None,
        };
    if let Some(quote_of_uri) = quote_of_uri.as_deref()
        && let Some(message) =
            validate_local_quote_creation(&db, &config, &access.account, &draft, quote_of_uri)
                .await?
    {
        return Response::error(message, 422);
    }
    if let Some(scheduled_at) = scheduled_at.as_deref() {
        if let Err(message) = validate_scheduled_at_minimum_offset(scheduled_at) {
            return Response::error(message, 422);
        }
        return Response::from_json(
            &crate::create_scheduled_status(
                &db,
                &config,
                &access.account.id,
                &draft,
                idempotency_key.as_deref(),
                access.application_id,
                quote_of_uri.as_deref(),
                scheduled_at,
            )
            .await?,
        );
    }

    let status = insert_status(
        &db,
        &config,
        &access.account,
        &draft,
        access.application_id,
        quote_of_uri.as_deref(),
    )
    .await?;
    ensure_direct_conversation_for_status(&db, &config, &access.account, &draft, &status).await?;
    attach_media_to_status(&db, &status.id, &pending_media).await?;
    let attached_media = find_media_attachments_by_status_id(&db, &status.id).await?;
    enqueue_outbox_activity(&db, &config, &access.account, &status).await?;
    let _ = send_status_quote_notification(&db, &config, &status).await;
    if let Some(recipient_account_id) = in_reply_to_account_id.as_deref()
        && recipient_account_id != access.account.id
    {
        let _ = send_push_notification(
            &db,
            &config,
            recipient_account_id,
            "status",
            serde_json::json!({
                "account_id": access.account.id,
                "status_id": status.id,
                "in_reply_to_account_id": recipient_account_id,
            }),
        )
        .await;
    }
    for handle in extract_mentions_from_text(&status._text_content, &config) {
        if let Some(account) = find_account_by_username(&db, &handle.username).await?
            && account.id != access.account.id
        {
            let _ = send_push_notification(
                &db,
                &config,
                &account.id,
                "mention",
                serde_json::json!({
                    "account_id": access.account.id,
                    "status_id": status.id,
                }),
            )
            .await;
        }
    }
    let response = build_local_status_response(
        &db,
        &config,
        Some(&access.account),
        &status,
        &access.account,
        in_reply_to_account_id,
        attached_media,
    )
    .await?;

    Response::from_json(&response)
}

pub(crate) async fn delete_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let query: DeleteStatusQuery = req.query().unwrap_or_default();

    let db = ctx.d1(&config.database_binding)?;
    let requester = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != requester.id {
        return Response::error("status not found", 404);
    }

    let media = find_media_attachments_by_status_id(&db, &status.id).await?;
    let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
    let mut response = MastodonStatusResponse::from_deleted_row(
        &status,
        &requester,
        &config,
        in_reply_to_account_id,
        media.clone(),
    );
    response.poll = load_mastodon_poll_response(&db, &status.id, Some(&requester)).await?;

    enqueue_outbox_delete(&db, &config, &requester, &status).await?;
    delete_status_by_id(&db, &status.id).await?;
    if query.delete_media.unwrap_or(false) {
        let bucket = ctx.bucket(&config.media_binding)?;
        delete_media_attachments(&db, &bucket, &media).await?;
    }

    Response::from_json(&response)
}

pub(crate) async fn update_status(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request: UpdateStatusRequest = match parse_update_status_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != account.id {
        return Response::error("status not found", 404);
    }
    let current_media = find_media_attachments_by_status_id(&db, &status.id).await?;

    let next_text = request
        .status
        .unwrap_or_else(|| status._text_content.clone());
    let next_spoiler_text = request
        .spoiler_text
        .unwrap_or_else(|| status.spoiler_text.clone());
    let next_sensitive = request.sensitive.unwrap_or(status.sensitive != 0);
    let next_language = request.language.or(status.language.clone());
    let next_poll = match request.poll {
        Some(poll) => match normalize_status_poll(Some(poll)) {
            Ok(poll) => poll,
            Err(message) => return Response::error(message, 422),
        },
        None => None,
    };
    let requested_media = match request.media_ids.as_ref() {
        Some(media_ids) => match resolve_editable_media(&db, &account, &status.id, media_ids).await
        {
            Ok(media) => Some(media),
            Err(message) => return Response::error(message, 422),
        },
        None => None,
    };
    let existing_poll = find_status_poll_by_status_id(&db, &status.id).await?;
    let resulting_media = requested_media.as_ref().unwrap_or(&current_media);
    if next_poll.is_some() && !resulting_media.is_empty() {
        return Response::error("poll and media_ids cannot be used together", 422);
    }
    if next_poll.is_some()
        && let Some(existing_poll) = existing_poll.as_ref()
    {
        let has_votes = crate::count_poll_voters(&db, &existing_poll.id).await? > 0
            || list_status_poll_options(&db, &existing_poll.id)
                .await?
                .into_iter()
                .any(|option| option.votes_count > 0);
        let expired = crate::is_iso_timestamp_in_past(&existing_poll.expires_at).unwrap_or(false);
        if has_votes || expired {
            return Response::error(
                "cannot edit a poll after voting has started or it has expired",
                422,
            );
        }
    }
    let changed = next_text != status._text_content
        || next_spoiler_text != status.spoiler_text
        || next_sensitive != (status.sensitive != 0)
        || next_language != status.language
        || next_poll.is_some()
        || requested_media.is_some()
        || request
            .media_attributes
            .as_ref()
            .is_some_and(|attributes| !attributes.is_empty());
    if next_text.is_empty()
        && resulting_media.is_empty()
        && existing_poll.is_none()
        && next_poll.is_none()
    {
        return Response::error("status must include text, media_ids, or poll", 422);
    }
    if !changed {
        let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
        let response = build_local_status_response(
            &db,
            &config,
            Some(&account),
            &status,
            &account,
            in_reply_to_account_id,
            current_media,
        )
        .await?;
        return Response::from_json(&response);
    }

    let previous_in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
    let previous_response = build_local_status_response(
        &db,
        &config,
        Some(&account),
        &status,
        &account,
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
    insert_status_edit_snapshot(&db, &status.id, &previous_snapshot_json, &revision_at).await?;

    let status = update_local_status(
        &db,
        &status,
        &next_text,
        &next_spoiler_text,
        next_sensitive,
        next_language.as_deref(),
        &revision_at,
    )
    .await?;
    if let Some(media) = requested_media.as_ref() {
        replace_status_media(&db, &status.id, media).await?;
    }
    if let Some(poll) = next_poll.as_ref() {
        replace_status_poll(&db, &status.id, poll, &revision_at).await?;
    }
    if let Some(attributes) = request.media_attributes.as_ref() {
        let attached_media = find_media_attachments_by_status_id(&db, &status.id).await?;
        for (index, attribute) in attributes.iter().enumerate() {
            let target_id = attribute
                .id
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| attached_media.get(index).map(|media| media.id.clone()));
            let Some(target_id) = target_id else {
                continue;
            };
            let Some(media) = attached_media.iter().find(|media| media.id == target_id) else {
                return Response::error(
                    format!("unknown media attachment in media_attributes: {target_id}"),
                    422,
                );
            };
            crate::apply_media_update(
                &db,
                media,
                UpdateMediaRequest {
                    description: attribute.description.clone(),
                    focus: attribute.focus.clone(),
                },
            )
            .await?;
        }
    }
    enqueue_status_update_activity(&db, &config, &account, &status).await?;
    let _ = send_status_update_notifications(&db, &config, &status).await;

    let media = find_media_attachments_by_status_id(&db, &status.id).await?;
    let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
    let response = build_local_status_response(
        &db,
        &config,
        Some(&account),
        &status,
        &account,
        in_reply_to_account_id,
        media,
    )
    .await?;

    Response::from_json(&response)
}
