use crate::{
    Error, Request, Response, Result, RouteContext, build_local_status_response,
    can_view_local_status, find_account_by_id, find_media_attachments_by_status_id,
    find_status_by_id, load_config, load_in_reply_to_account_id,
    require_authenticated_local_account,
};
use std::collections::HashSet;
use worker::d1::D1Type;

async fn resolve_thread_root_status_id(
    db: &worker::D1Database,
    status: &crate::StatusRow,
) -> Result<String> {
    let mut current_id = status.id.clone();
    let mut parent_id = status.in_reply_to_id.clone();
    let mut seen = HashSet::new();

    loop {
        if !seen.insert(current_id.clone()) {
            return Err(Error::RustError(
                "detected cycle while resolving thread root".to_owned(),
            ));
        }
        let Some(next_parent_id) = parent_id.as_deref() else {
            return Ok(current_id);
        };
        let Some(parent) = find_status_by_id(db, next_parent_id).await? else {
            return Ok(current_id);
        };
        current_id = parent.id;
        parent_id = parent.in_reply_to_id;
    }
}

pub(crate) async fn is_local_status_thread_muted_by(
    db: &worker::D1Database,
    account_id: &str,
    status: &crate::StatusRow,
) -> Result<bool> {
    let root_status_id = resolve_thread_root_status_id(db, status).await?;
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(root_status_id.as_str()),
    ];
    Ok(db
        .prepare(
            "SELECT thread_root_status_id
             FROM thread_mutes
             WHERE account_id = ?1
               AND thread_root_status_id = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?
        .is_some())
}

async fn mute_thread_for_status(
    db: &worker::D1Database,
    account_id: &str,
    status: &crate::StatusRow,
) -> Result<()> {
    let root_status_id = resolve_thread_root_status_id(db, status).await?;
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(root_status_id.as_str()),
    ];
    db.prepare(
        "INSERT INTO thread_mutes (account_id, thread_root_status_id)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, thread_root_status_id) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn unmute_thread_for_status(
    db: &worker::D1Database,
    account_id: &str,
    status: &crate::StatusRow,
) -> Result<()> {
    let root_status_id = resolve_thread_root_status_id(db, status).await?;
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(root_status_id.as_str()),
    ];
    db.prepare(
        "DELETE FROM thread_mutes
         WHERE account_id = ?1
           AND thread_root_status_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn mute_status_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    let Some(author) = find_account_by_id(&db, &status.account_id).await? else {
        return Response::error("status not found", 404);
    };
    if !can_view_local_status(&db, &status, Some(&viewer), &author).await? {
        return Response::error("status not found", 404);
    }

    mute_thread_for_status(&db, &viewer.id, &status).await?;
    let media = find_media_attachments_by_status_id(&db, &status.id).await?;
    Response::from_json(
        &build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &status,
            &author,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?,
    )
}

pub(crate) async fn unmute_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    let Some(author) = find_account_by_id(&db, &status.account_id).await? else {
        return Response::error("status not found", 404);
    };
    if !can_view_local_status(&db, &status, Some(&viewer), &author).await? {
        return Response::error("status not found", 404);
    }

    unmute_thread_for_status(&db, &viewer.id, &status).await?;
    let media = find_media_attachments_by_status_id(&db, &status.id).await?;
    Response::from_json(
        &build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &status,
            &author,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?,
    )
}
