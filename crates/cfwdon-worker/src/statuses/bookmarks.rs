use super::{
    Request, Response, Result, RouteContext, build_local_action_status_response,
    build_remote_status_response, build_saved_status_collection_response,
    delete_bookmark_by_target_uri, list_bookmarks_for_account, local_status_target_uri,
    resolve_authenticated_status_action_context, resolve_authenticated_status_viewer_context,
    resolve_visible_action_status, upsert_bookmark_local_status, upsert_bookmark_remote_status,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct BookmarksQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) _max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) _since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) _min_id: Option<String>,
}

pub(crate) async fn bookmark_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
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
            upsert_bookmark_local_status(&action.auth.db, &viewer.id, &subject.status).await?;
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
            upsert_bookmark_remote_status(&action.auth.db, &viewer.id, &status).await?;
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
        crate::AuthenticatedStatusActionContextResolution::Ready(action) => action,
        crate::AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        crate::AuthenticatedStatusActionContextResolution::Unauthenticated => {
            return Response::error("Cloudflare Access authentication required", 401);
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
            delete_bookmark_by_target_uri(
                &action.auth.db,
                &viewer.id,
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
        Some(crate::ResolvedVisibleActionStatus::Remote(status, actor)) => {
            delete_bookmark_by_target_uri(&action.auth.db, &viewer.id, &status.object_uri).await?;
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
    let query: BookmarksQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let Some(auth) = resolve_authenticated_status_viewer_context(&req, &ctx).await? else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let bookmark_entries =
        list_bookmarks_for_account(&auth.db, &auth.viewer.id, limit.saturating_mul(3)).await?;
    build_saved_status_collection_response(
        &auth.db,
        &auth.config,
        &auth.viewer,
        &bookmark_entries,
        limit,
        |entry| &entry.created_at,
        |entry| entry.status_id.as_deref(),
        |entry| entry.remote_status_id.as_deref(),
    )
    .await
}
