use crate::{
    Request, Response, Result, RouteContext, build_local_status_response,
    enqueue_add_featured_status_activity, enqueue_remove_featured_status_activity,
    extract_authenticated_user, find_media_attachments_by_status_id, find_status_by_id,
    load_config, load_in_reply_to_account_id, resolve_local_account,
};
use worker::d1::D1Type;

pub(crate) async fn is_local_status_pinned_by(
    db: &worker::D1Database,
    account_id: &str,
    status_id: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(status_id)];
    Ok(db
        .prepare(
            "SELECT status_id
             FROM status_pins
             WHERE account_id = ?1
               AND status_id = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?
        .is_some())
}

pub(crate) async fn pin_local_status(
    db: &worker::D1Database,
    account_id: &str,
    status_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(status_id)];
    db.prepare(
        "INSERT INTO status_pins (account_id, status_id)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, status_id) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn unpin_local_status(
    db: &worker::D1Database,
    account_id: &str,
    status_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(status_id)];
    db.prepare(
        "DELETE FROM status_pins
         WHERE account_id = ?1
           AND status_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn list_pinned_statuses_for_account(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<Vec<crate::StatusRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
             FROM status_pins sp
             JOIN statuses s
               ON s.id = sp.status_id
             WHERE sp.account_id = ?1
             ORDER BY sp.created_at DESC, s.created_at DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<crate::StatusRow>()
}

async fn pinned_status_response(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
    status: &crate::StatusRow,
) -> Result<crate::MastodonStatusResponse> {
    let author = crate::find_account_by_id(db, &status.account_id)
        .await?
        .ok_or_else(|| worker::Error::RustError("status author not found".to_owned()))?;
    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    let in_reply_to_account_id = load_in_reply_to_account_id(db, status).await?;
    build_local_status_response(
        db,
        config,
        Some(viewer),
        status,
        &author,
        in_reply_to_account_id,
        media,
    )
    .await
}

pub(crate) async fn pin_status_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != viewer.id {
        return Response::error("status not found", 404);
    }

    pin_local_status(&db, &viewer.id, &status.id).await?;
    enqueue_add_featured_status_activity(&db, &config, &viewer, &status).await?;
    Response::from_json(&pinned_status_response(&db, &config, &viewer, &status).await?)
}

pub(crate) async fn unpin_status_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != viewer.id {
        return Response::error("status not found", 404);
    }

    unpin_local_status(&db, &viewer.id, &status.id).await?;
    enqueue_remove_featured_status_activity(&db, &config, &viewer, &status).await?;
    Response::from_json(&pinned_status_response(&db, &config, &viewer, &status).await?)
}
