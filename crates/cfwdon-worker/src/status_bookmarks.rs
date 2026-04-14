use super::{
    Request, Response, Result, RouteContext, build_local_status_response,
    build_remote_status_response, can_view_local_status, delete_bookmark_by_target_uri,
    find_account_by_id, find_authenticated_local_account, find_media_attachments_by_status_id,
    find_remote_actor_by_actor_uri, find_remote_status_by_id, find_status_by_id,
    is_public_activitypub_visibility, list_bookmarks_for_account, load_config,
    load_in_reply_to_account_id, local_status_target_uri, status_id_from_context,
    upsert_bookmark_local_status, upsert_bookmark_remote_status,
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
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("status not found", 404);
        }
        upsert_bookmark_local_status(&db, &viewer.id, &status).await?;
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
        return Response::from_json(&response);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        upsert_bookmark_remote_status(&db, &viewer.id, &status).await?;
        let response =
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
        return Response::from_json(&response);
    }

    Response::error("status not found", 404)
}

pub(crate) async fn unbookmark_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("status not found", 404);
        }
        delete_bookmark_by_target_uri(&db, &viewer.id, &local_status_target_uri(&status)).await?;
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
        return Response::from_json(&response);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        delete_bookmark_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
        let response =
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
        return Response::from_json(&response);
    }

    Response::error("status not found", 404)
}

pub(crate) async fn bookmarks_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: BookmarksQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mut entries = Vec::new();
    for entry in list_bookmarks_for_account(&db, &viewer.id, limit.saturating_mul(3)).await? {
        if let Some(status_id) = entry.status_id.as_deref()
            && let Some(status) = find_status_by_id(&db, status_id).await?
            && let Some(account) = find_account_by_id(&db, &status.account_id).await?
        {
            if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
                continue;
            }
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
            entries.push((
                entry.created_at,
                serde_json::to_value(response).unwrap_or_default(),
            ));
            continue;
        }

        if let Some(remote_status_id) = entry.remote_status_id.as_deref()
            && let Some(status) = find_remote_status_by_id(&db, remote_status_id).await?
            && let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await?
        {
            let response =
                build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
            entries.push((
                entry.created_at,
                serde_json::to_value(response).unwrap_or_default(),
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Response::from_json(
        &entries
            .into_iter()
            .map(|(_, value)| value)
            .take(limit as usize)
            .collect::<Vec<_>>(),
    )
}
