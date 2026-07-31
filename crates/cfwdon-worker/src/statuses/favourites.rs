use super::{
    Request, Response, Result, RouteContext, build_like_activity,
    build_local_action_status_response, build_remote_status_response,
    build_saved_status_collection_response, build_undo_like_activity,
    delete_favourite_by_target_uri, find_favourite_activity_by_target_uri,
    invalidate_status_api_cache, list_favourites_for_account, local_status_target_uri,
    queue_remote_actor_activity, resolve_authenticated_status_action_context,
    resolve_authenticated_status_viewer_context, resolve_visible_action_status,
    upsert_favourite_local_status, upsert_favourite_remote_status,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FavouritesQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) _max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) _since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) _min_id: Option<String>,
}

pub(crate) async fn favourite_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let action = match resolve_authenticated_status_action_context(&req, &ctx).await? {
        crate::AuthenticatedStatusActionContextResolution::Ready(action) => action,
        crate::AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        crate::AuthenticatedStatusActionContextResolution::Unauthenticated => {
            return Response::error("Auth0 authentication required", 401);
        }
    };
    let viewer = &action.auth.viewer;

    match resolve_visible_action_status(
        &action.auth.db,
        &action.auth.config,
        viewer,
        &action.status_id,
        action.action_uri.as_deref(),
    )
    .await?
    {
        Some(crate::ResolvedVisibleActionStatus::Local(subject)) => {
            upsert_favourite_local_status(
                &action.auth.db,
                &action.auth.config,
                Some(&ctx.env),
                viewer,
                &subject.status,
            )
            .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            let response = build_local_action_status_response(
                &action.auth.db,
                &action.auth.config,
                viewer,
                subject,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(crate::ResolvedVisibleActionStatus::Remote(status, actor)) => {
            let existing = find_favourite_activity_by_target_uri(
                &action.auth.db,
                viewer.id(),
                &status.object_uri,
            )
            .await?;
            let mut outbound_activity_id = existing.and_then(|row| row.ap_activity_id);
            if outbound_activity_id.is_none() {
                let (_, payload_json) = build_like_activity(
                    &action.auth.config,
                    viewer,
                    &actor.actor_uri,
                    &status.object_uri,
                )?;
                outbound_activity_id = queue_remote_actor_activity(
                    &action.auth.db,
                    viewer.id(),
                    &actor.actor_uri,
                    &payload_json,
                )
                .await?;
            }
            upsert_favourite_remote_status(
                &action.auth.db,
                viewer.id(),
                &status,
                outbound_activity_id.as_deref(),
            )
            .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
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

pub(crate) async fn unfavourite_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let action = match resolve_authenticated_status_action_context(&req, &ctx).await? {
        crate::AuthenticatedStatusActionContextResolution::Ready(action) => action,
        crate::AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        crate::AuthenticatedStatusActionContextResolution::Unauthenticated => {
            return Response::error("Auth0 authentication required", 401);
        }
    };
    let viewer = &action.auth.viewer;

    match resolve_visible_action_status(
        &action.auth.db,
        &action.auth.config,
        viewer,
        &action.status_id,
        action.action_uri.as_deref(),
    )
    .await?
    {
        Some(crate::ResolvedVisibleActionStatus::Local(subject)) => {
            delete_favourite_by_target_uri(
                &action.auth.db,
                viewer.id(),
                &local_status_target_uri(&subject.status),
            )
            .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            let response = build_local_action_status_response(
                &action.auth.db,
                &action.auth.config,
                viewer,
                subject,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(crate::ResolvedVisibleActionStatus::Remote(status, actor)) => {
            if let Some(row) = find_favourite_activity_by_target_uri(
                &action.auth.db,
                viewer.id(),
                &status.object_uri,
            )
            .await?
                && let Some(like_activity_id) = row.ap_activity_id.as_deref()
            {
                let (_, payload_json) = build_undo_like_activity(
                    &action.auth.config,
                    viewer,
                    like_activity_id,
                    &actor.actor_uri,
                    &status.object_uri,
                )?;
                let _ = queue_remote_actor_activity(
                    &action.auth.db,
                    viewer.id(),
                    &actor.actor_uri,
                    &payload_json,
                )
                .await?;
            }
            delete_favourite_by_target_uri(&action.auth.db, viewer.id(), &status.object_uri)
                .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
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

pub(crate) async fn favourites_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let query: FavouritesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let Some(auth) = resolve_authenticated_status_viewer_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };

    let favourite_entries =
        list_favourites_for_account(&auth.db, auth.viewer.id(), limit.saturating_mul(3)).await?;
    build_saved_status_collection_response(
        &auth.db,
        &auth.config,
        &auth.viewer,
        &favourite_entries,
        limit,
        |entry| &entry.created_at,
        |entry| entry.status_id.as_deref(),
        |entry| entry.remote_status_id.as_deref(),
    )
    .await
}
