use super::{
    DeleteStatusQuery, MastodonStatusResponse, Request, Response, Result, RouteContext,
    attach_media_to_status, build_local_status_response, delete_media_attachments,
    delete_status_by_id, enqueue_outbox_activity, enqueue_outbox_delete,
    extract_authenticated_user, find_authenticated_local_account,
    find_media_attachments_by_status_id, find_status_by_id, insert_status, load_config,
    load_in_reply_to_account_id, load_mastodon_poll_response, parse_status_draft,
    resolve_attachable_media, resolve_local_account, status_id_from_context,
};

pub(crate) async fn create_status(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let draft = match parse_status_draft(&mut req).await {
        Ok(draft) => draft,
        Err(message) => return Response::error(message, 422),
    };
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

    let status = insert_status(&db, &config, &account, &draft).await?;
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
