use super::{
    Error, Request, Response, Result, RouteContext, build_activitypub_note,
    build_local_status_context, build_local_status_response, build_remote_status_context,
    can_view_local_status, find_account_by_id, find_account_by_username,
    find_authenticated_local_account, find_media_attachments_by_status_id,
    find_remote_actor_by_actor_uri, find_remote_status_by_id, find_status_by_id,
    is_public_activitypub_visibility, load_config, load_in_reply_to_account_id,
    status_id_from_context,
};

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

pub(crate) async fn status_api_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
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

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(owner) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, viewer.as_ref(), &owner).await? {
            return Response::error("status not found", 404);
        }

        return Response::from_json(
            &build_local_status_context(&db, &config, viewer.as_ref(), &status, &owner).await?,
        );
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        return Response::from_json(
            &build_remote_status_context(&db, &config, &status, &actor).await?,
        );
    }

    Response::error("status not found", 404)
}
