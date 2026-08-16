use super::{
    LocalStatusResponsePreload, Request, Response, Result, RouteContext, UpdateLocalStatusInput,
    UpdateStatusRequest, apply_local_status_update, build_local_status_response,
    config_with_resolved_custom_emojis, count_poll_voters, find_authenticated_local_account,
    find_owned_local_status, find_status_poll_by_status_id, invalidate_status_api_cache,
    is_iso_timestamp_in_past, list_status_poll_options, load_config,
    load_local_status_response_preload, normalize_status_poll, parse_update_status_request,
    resolve_editable_media, sanitize_emoji_shortcodes, status_id_from_context,
};
use worker::Error;

pub(crate) async fn update_status(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let request: UpdateStatusRequest = match parse_update_status_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let db = crate::bind_request_d1(&ctx, &config)?;
    let config = config_with_resolved_custom_emojis(&db, &config).await?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(status) = find_owned_local_status(&db, &status_id, account.id()).await? else {
        return Response::error("status not found", 404);
    };
    let LocalStatusResponsePreload {
        media: current_media,
        in_reply_to_account_id: current_in_reply_to_account_id,
    } = load_local_status_response_preload(&db, &status).await?;

    let next_text = sanitize_emoji_shortcodes(
        &request.status.unwrap_or_else(|| status.text.clone()),
        &config,
    );
    let next_spoiler_text = sanitize_emoji_shortcodes(
        &request
            .spoiler_text
            .unwrap_or_else(|| status.spoiler_text.clone()),
        &config,
    );
    let next_sensitive = request.sensitive.unwrap_or(status.sensitive);
    let next_language = request.language.or(status.language.clone());
    let next_poll = match request.poll {
        Some(poll) => match normalize_status_poll(Some(poll), &config) {
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
        let has_votes = count_poll_voters(&db, &existing_poll.id).await? > 0
            || list_status_poll_options(&db, &existing_poll.id)
                .await?
                .into_iter()
                .any(|option| option.votes_count > 0);
        let expired = is_iso_timestamp_in_past(&existing_poll.expires_at).unwrap_or(false);
        if has_votes || expired {
            return Response::error(
                "cannot edit a poll after voting has started or it has expired",
                422,
            );
        }
    }
    let changed = next_text != status.text
        || next_spoiler_text != status.spoiler_text
        || next_sensitive != (status.sensitive)
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
        let response = build_local_status_response(
            &db,
            &config,
            Some(&account),
            &status,
            &account,
            current_in_reply_to_account_id,
            current_media,
        )
        .await?;
        return Response::from_json(&response);
    }

    let updated = match apply_local_status_update(
        &db,
        &config,
        Some(&ctx.env),
        UpdateLocalStatusInput {
            account: &account,
            status: &status,
            current_media: current_media.clone(),
            current_in_reply_to_account_id,
            next_text: &next_text,
            next_spoiler_text: &next_spoiler_text,
            next_sensitive,
            next_language: next_language.as_deref(),
            next_poll: next_poll.as_ref(),
            requested_media: requested_media.as_deref(),
            media_attributes: request.media_attributes.as_deref(),
        },
    )
    .await
    {
        Ok(updated) => updated,
        Err(Error::RustError(message))
            if message.starts_with("unknown media attachment in media_attributes:") =>
        {
            return Response::error(message, 422);
        }
        Err(error) => return Err(error),
    };
    invalidate_status_api_cache(&ctx, &updated.status_id).await;
    Response::from_json(&updated.response)
}
