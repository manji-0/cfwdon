use super::{
    DeleteStatusQuery, MastodonStatusResponse, Request, Response, Result, RouteContext,
    UpdateMediaRequest, UpdateStatusRequest, attach_media_to_status, build_local_status_response,
    delete_media_attachments, delete_status_by_id, enqueue_outbox_activity, enqueue_outbox_delete,
    enqueue_status_update_activity, ensure_direct_conversation_for_status,
    extract_authenticated_user, find_authenticated_local_account, find_local_status_by_object_uri,
    find_media_attachments_by_status_id, find_remote_status_by_id,
    find_remote_status_by_url_or_object_uri, find_status_by_id, find_status_poll_by_status_id,
    insert_status, insert_status_edit_snapshot, list_status_poll_options, load_config,
    load_in_reply_to_account_id, load_mastodon_poll_response, normalize_status_history_entry,
    normalize_status_poll, now_iso_string, parse_status_draft, parse_update_status_request,
    replace_status_media, replace_status_poll, resolve_attachable_media, resolve_editable_media,
    resolve_local_account, status_id_from_context, update_local_status,
};

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

pub(crate) async fn create_status(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let parsed = match parse_status_draft(&mut req).await {
        Ok(draft) => draft,
        Err(message) => return Response::error(message, 422),
    };
    let super::ParsedStatusDraft {
        draft,
        quoted_status_id,
    } = parsed;
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let pending_media = match resolve_attachable_media(&db, &account, &draft.media_ids).await {
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

    let status = insert_status(&db, &config, &account, &draft, quote_of_uri.as_deref()).await?;
    ensure_direct_conversation_for_status(&db, &config, &account, &draft, &status).await?;
    attach_media_to_status(&db, &status.id, &pending_media).await?;
    let attached_media = find_media_attachments_by_status_id(&db, &status.id).await?;
    enqueue_outbox_activity(&db, &config, &account, &status).await?;
    let response = build_local_status_response(
        &db,
        &config,
        Some(&account),
        &status,
        &account,
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
