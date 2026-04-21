use super::{
    Error, Request, Response, Result, RouteContext, StatusActionQuery, build_announce_activity,
    build_local_status_response, build_remote_status_response, build_undo_announce_activity,
    can_view_local_status, delete_reblog_by_target_uri, delete_reblog_wrapper_status_by_target_uri,
    find_authenticated_local_account, find_media_attachments_by_status_id,
    find_reblog_activity_by_target_uri, is_public_activitypub_visibility, load_config,
    load_in_reply_to_account_id, local_status_target_uri, queue_remote_actor_activity,
    resolve_action_status, status_id_from_context, upsert_reblog_local_status,
    upsert_reblog_remote_status, upsert_reblog_wrapper_status,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ReblogStatusRequest {
    pub(crate) visibility: Option<String>,
}

pub(crate) async fn reblog_status(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let path_status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let action_query: StatusActionQuery = req.query().unwrap_or_default();
    if action_query.uri.is_some()
        && crate::normalized_action_uri(action_query.uri.as_deref()).is_none()
    {
        return Response::error("uri query parameter must not be empty", 400);
    }
    let request = parse_reblog_status_request(req)
        .await
        .map_err(Error::RustError)?;
    let visibility = request.visibility.unwrap_or_else(|| "public".to_owned());
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    match resolve_action_status(&db, &config, &path_status_id, action_query.uri.as_deref()).await? {
        Some(crate::ResolvedActionStatus::Local(status, account)) => {
            if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
                return Response::error("status not found", 404);
            }
            if viewer.id == account.id {
                return Response::error("cannot reblog your own status", 422);
            }
            upsert_reblog_local_status(&db, &config, &viewer.id, &status, &visibility).await?;
            let wrapper = upsert_reblog_wrapper_status(
                &db,
                &config,
                &viewer,
                &local_status_target_uri(&status),
                &visibility,
            )
            .await?;
            let media = find_media_attachments_by_status_id(&db, &wrapper.id).await?;
            let response = build_local_status_response(
                &db,
                &config,
                Some(&viewer),
                &wrapper,
                &viewer,
                load_in_reply_to_account_id(&db, &wrapper).await?,
                media,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(crate::ResolvedActionStatus::Remote(status, actor)) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            let existing =
                find_reblog_activity_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
            let mut outbound_activity_id = existing.and_then(|row| row.ap_activity_id);
            if outbound_activity_id.is_none() {
                let (_, payload_json) =
                    build_announce_activity(&config, &viewer, &status.object_uri, &visibility)?;
                outbound_activity_id =
                    queue_remote_actor_activity(&db, &viewer.id, &actor.actor_uri, &payload_json)
                        .await?;
            }
            upsert_reblog_remote_status(
                &db,
                &viewer.id,
                &status,
                &visibility,
                outbound_activity_id.as_deref(),
            )
            .await?;
            let wrapper = upsert_reblog_wrapper_status(
                &db,
                &config,
                &viewer,
                &status.object_uri,
                &visibility,
            )
            .await?;
            let media = find_media_attachments_by_status_id(&db, &wrapper.id).await?;
            let response = build_local_status_response(
                &db,
                &config,
                Some(&viewer),
                &wrapper,
                &viewer,
                load_in_reply_to_account_id(&db, &wrapper).await?,
                media,
            )
            .await?;
            Response::from_json(&response)
        }
        None => Response::error("status not found", 404),
    }
}

pub(crate) async fn unreblog_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let action_query: StatusActionQuery = req.query().unwrap_or_default();
    if action_query.uri.is_some()
        && crate::normalized_action_uri(action_query.uri.as_deref()).is_none()
    {
        return Response::error("uri query parameter must not be empty", 400);
    }
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    match resolve_action_status(&db, &config, &status_id, action_query.uri.as_deref()).await? {
        Some(crate::ResolvedActionStatus::Local(status, account)) => {
            if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
                return Response::error("status not found", 404);
            }
            delete_reblog_by_target_uri(&db, &viewer.id, &local_status_target_uri(&status)).await?;
            delete_reblog_wrapper_status_by_target_uri(
                &db,
                &viewer.id,
                &local_status_target_uri(&status),
            )
            .await?;
            let media = find_media_attachments_by_status_id(&db, &status.id).await?;
            let response = build_local_status_response(
                &db,
                &config,
                Some(&viewer),
                &status,
                &account,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(crate::ResolvedActionStatus::Remote(status, actor)) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            if let Some(row) =
                find_reblog_activity_by_target_uri(&db, &viewer.id, &status.object_uri).await?
                && let Some(announce_activity_id) = row.ap_activity_id.as_deref()
            {
                let (_, payload_json) = build_undo_announce_activity(
                    &config,
                    &viewer,
                    announce_activity_id,
                    &actor.actor_uri,
                    &status.object_uri,
                    &row.visibility,
                )?;
                let _ =
                    queue_remote_actor_activity(&db, &viewer.id, &actor.actor_uri, &payload_json)
                        .await?;
            }
            delete_reblog_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
            delete_reblog_wrapper_status_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
            let response =
                build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
            Response::from_json(&response)
        }
        None => Response::error("status not found", 404),
    }
}

async fn parse_reblog_status_request(
    req: &mut Request,
) -> std::result::Result<ReblogStatusRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.trim().is_empty() {
        ReblogStatusRequest::default()
    } else if content_type.contains("application/json") {
        req.json::<ReblogStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON reblog payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form reblog payload: {error}"))?;
        ReblogStatusRequest {
            visibility: form.get_field("visibility"),
        }
    };

    if let Some(visibility) = request.visibility.as_mut() {
        *visibility = visibility.trim().to_ascii_lowercase();
        if visibility.is_empty() {
            request.visibility = None;
        } else if super::Visibility::parse(visibility).is_none() {
            return Err("visibility must be one of: public, unlisted, private, direct".to_owned());
        }
    }

    Ok(request)
}
