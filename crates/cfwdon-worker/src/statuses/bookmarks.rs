use super::{
    SavedStatusesPage, build_local_action_status_response, build_remote_status_response,
    build_saved_status_collection_response, delete_bookmark_by_target_uri,
    list_bookmarks_for_account, local_status_target_uri,
    resolve_authenticated_status_action_context, resolve_authenticated_status_viewer_context,
    resolve_visible_action_status, upsert_bookmark_local_status, upsert_bookmark_remote_status,
};
use crate::statuses::{AuthenticatedStatusActionContextResolution, ResolvedVisibleActionStatus};
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn bookmark_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
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
            upsert_bookmark_local_status(&action.auth.db, viewer.id(), &subject.status).await?;
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
            upsert_bookmark_remote_status(&action.auth.db, viewer.id(), &status).await?;
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

pub(crate) async fn unbookmark_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
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
            delete_bookmark_by_target_uri(
                &action.auth.db,
                viewer.id(),
                &local_status_target_uri(&subject.status),
            )
            .await?;
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
            delete_bookmark_by_target_uri(&action.auth.db, viewer.id(), &status.object_uri).await?;
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

pub(crate) async fn bookmarks_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let page = match SavedStatusesPage::from_request(&req) {
        Ok(page) => page,
        Err(error) => return Response::error(error.to_string(), 400),
    };
    let Some(auth) = resolve_authenticated_status_viewer_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };

    let bookmark_entries = list_bookmarks_for_account(&auth.db, auth.viewer.id(), page).await?;
    build_saved_status_collection_response(
        &auth.db,
        &auth.config,
        &auth.viewer,
        &req,
        page,
        &bookmark_entries,
        |entry| entry.cursor_id,
        |entry| entry.status_id.as_deref(),
        |entry| entry.remote_status_id.as_deref(),
    )
    .await
}
