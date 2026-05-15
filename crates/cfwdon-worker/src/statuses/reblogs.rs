use super::{
    Error, Request, Response, Result, RouteContext, build_announce_activity,
    build_loaded_local_status_response, build_remote_status_response, build_undo_announce_activity,
    can_view_local_status, delete_reblog_by_target_uri, delete_reblog_wrapper_status_by_target_uri,
    find_reblog_activity_by_target_uri, invalidate_status_api_cache,
    is_public_activitypub_visibility, local_status_target_uri, queue_remote_actor_activity,
    resolve_action_status, resolve_authenticated_status_action_context, upsert_reblog_local_status,
    upsert_reblog_remote_status, upsert_reblog_wrapper_status,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ReblogStatusRequest {
    pub(crate) visibility: Option<String>,
}

pub(crate) async fn reblog_status(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let action = match resolve_authenticated_status_action_context(&*req, &ctx).await? {
        crate::AuthenticatedStatusActionContextResolution::Ready(action) => action,
        crate::AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        crate::AuthenticatedStatusActionContextResolution::Unauthenticated => {
            return Response::error("Cloudflare Access authentication required", 401);
        }
    };
    let request = parse_reblog_status_request(req)
        .await
        .map_err(Error::RustError)?;
    let visibility = request.visibility.unwrap_or_else(|| "public".to_owned());
    let viewer = &action.auth.viewer;

    match resolve_action_status(
        &action.auth.db,
        &action.auth.config,
        &action.status_id,
        action.action_uri.as_deref(),
    )
    .await?
    {
        Some(crate::ResolvedActionStatus::Local(status, account)) => {
            if !can_view_local_status(&action.auth.db, &status, Some(viewer), &account).await? {
                return Response::error("status not found", 404);
            }
            if viewer.id == account.id {
                return Response::error("cannot reblog your own status", 422);
            }
            upsert_reblog_local_status(
                &action.auth.db,
                &action.auth.config,
                &viewer.id,
                &status,
                &visibility,
            )
            .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            let wrapper = upsert_reblog_wrapper_status(
                &action.auth.db,
                &action.auth.config,
                viewer,
                &local_status_target_uri(&status),
                &visibility,
            )
            .await?;
            let response = build_loaded_local_status_response(
                &action.auth.db,
                &action.auth.config,
                Some(viewer),
                &wrapper,
                viewer,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(crate::ResolvedActionStatus::Remote(status, actor)) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            let existing =
                find_reblog_activity_by_target_uri(&action.auth.db, &viewer.id, &status.object_uri)
                    .await?;
            let mut outbound_activity_id = existing.and_then(|row| row.ap_activity_id);
            if outbound_activity_id.is_none() {
                let (_, payload_json) = build_announce_activity(
                    &action.auth.config,
                    viewer,
                    &status.object_uri,
                    &visibility,
                )?;
                outbound_activity_id = queue_remote_actor_activity(
                    &action.auth.db,
                    &viewer.id,
                    &actor.actor_uri,
                    &payload_json,
                )
                .await?;
            }
            upsert_reblog_remote_status(
                &action.auth.db,
                &viewer.id,
                &status,
                &visibility,
                outbound_activity_id.as_deref(),
            )
            .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            let wrapper = upsert_reblog_wrapper_status(
                &action.auth.db,
                &action.auth.config,
                viewer,
                &status.object_uri,
                &visibility,
            )
            .await?;
            let response = build_loaded_local_status_response(
                &action.auth.db,
                &action.auth.config,
                Some(viewer),
                &wrapper,
                viewer,
            )
            .await?;
            Response::from_json(&response)
        }
        None => Response::error("status not found", 404),
    }
}

pub(crate) async fn unreblog_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let action = match resolve_authenticated_status_action_context(&req, &ctx).await? {
        crate::AuthenticatedStatusActionContextResolution::Ready(action) => action,
        crate::AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        crate::AuthenticatedStatusActionContextResolution::Unauthenticated => {
            return Response::error("Cloudflare Access authentication required", 401);
        }
    };
    let viewer = &action.auth.viewer;

    match resolve_action_status(
        &action.auth.db,
        &action.auth.config,
        &action.status_id,
        action.action_uri.as_deref(),
    )
    .await?
    {
        Some(crate::ResolvedActionStatus::Local(status, account)) => {
            if !can_view_local_status(&action.auth.db, &status, Some(viewer), &account).await? {
                return Response::error("status not found", 404);
            }
            delete_reblog_by_target_uri(
                &action.auth.db,
                &viewer.id,
                &local_status_target_uri(&status),
            )
            .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            delete_reblog_wrapper_status_by_target_uri(
                &action.auth.db,
                &viewer.id,
                &local_status_target_uri(&status),
            )
            .await?;
            let response = build_loaded_local_status_response(
                &action.auth.db,
                &action.auth.config,
                Some(viewer),
                &status,
                &account,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(crate::ResolvedActionStatus::Remote(status, actor)) => {
            if !is_public_activitypub_visibility(&status.visibility) {
                return Response::error("status not found", 404);
            }
            if let Some(row) =
                find_reblog_activity_by_target_uri(&action.auth.db, &viewer.id, &status.object_uri)
                    .await?
                && let Some(announce_activity_id) = row.ap_activity_id.as_deref()
            {
                let (_, payload_json) = build_undo_announce_activity(
                    &action.auth.config,
                    viewer,
                    announce_activity_id,
                    &actor.actor_uri,
                    &status.object_uri,
                    &row.visibility,
                )?;
                let _ = queue_remote_actor_activity(
                    &action.auth.db,
                    &viewer.id,
                    &actor.actor_uri,
                    &payload_json,
                )
                .await?;
            }
            delete_reblog_by_target_uri(&action.auth.db, &viewer.id, &status.object_uri).await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            delete_reblog_wrapper_status_by_target_uri(
                &action.auth.db,
                &viewer.id,
                &status.object_uri,
            )
            .await?;
            let response = build_remote_status_response(
                &action.auth.db,
                &action.auth.config,
                Some(viewer),
                &status,
                &actor,
            )
            .await?;
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
