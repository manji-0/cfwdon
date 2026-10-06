use super::{
    SavedStatusesPage, build_local_action_status_response, build_remote_status_response,
    build_saved_status_collection_response, delete_favourite_by_target_uri,
    find_favourite_activity_by_target_uri, list_favourites_for_account, local_status_target_uri,
    resolve_authenticated_status_action_context, resolve_authenticated_status_viewer_context,
    resolve_visible_action_status, upsert_favourite_local_status, upsert_favourite_remote_status,
};
use crate::activitypub::{build_like_activity, build_undo_like_activity};
use crate::delivery::queue_remote_actor_activity;
use crate::response_cache::invalidate_status_api_cache;
use crate::statuses::{AuthenticatedStatusActionContextResolution, ResolvedVisibleActionStatus};
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn favourite_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let action = match resolve_authenticated_status_action_context(&req, &ctx).await? {
        AuthenticatedStatusActionContextResolution::Ready(action) => action,
        AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        AuthenticatedStatusActionContextResolution::Unauthenticated => {
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
        Some(ResolvedVisibleActionStatus::Local(subject)) => {
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
        Some(ResolvedVisibleActionStatus::Remote(status, actor)) => {
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
        AuthenticatedStatusActionContextResolution::Ready(action) => action,
        AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        AuthenticatedStatusActionContextResolution::Unauthenticated => {
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
        Some(ResolvedVisibleActionStatus::Local(subject)) => {
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
        Some(ResolvedVisibleActionStatus::Remote(status, actor)) => {
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
    let page = match SavedStatusesPage::from_request(&req) {
        Ok(page) => page,
        Err(error) => return Response::error(error.to_string(), 400),
    };
    let Some(auth) = resolve_authenticated_status_viewer_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };

    let favourite_entries = list_favourites_for_account(&auth.db, auth.viewer.id(), page).await?;
    build_saved_status_collection_response(
        &auth.db,
        &auth.config,
        &auth.viewer,
        &req,
        page,
        &favourite_entries,
        |entry| entry.cursor_id,
        |entry| entry.status_id.as_deref(),
        |entry| entry.remote_status_id.as_deref(),
    )
    .await
}
