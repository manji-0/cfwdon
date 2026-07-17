use super::{
    Request, Response, Result, RouteContext, build_local_status_response,
    enqueue_add_featured_status_activity, enqueue_outbox_process_queue_best_effort,
    enqueue_remove_featured_status_activity, find_owned_local_status_response_subject, load_config,
    require_authenticated_local_account,
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
    result
        .results::<crate::StatusRecord>()
        .and_then(crate::statuses_from_records)
}

async fn pinned_status_response(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
    subject: super::LoadedLocalStatusResponseSubject,
) -> Result<crate::MastodonStatusResponse> {
    let super::LoadedLocalStatusResponseSubject {
        status,
        account,
        preload,
    } = subject;
    build_local_status_response(
        db,
        config,
        Some(viewer),
        &status,
        &account,
        preload.in_reply_to_account_id,
        preload.media,
    )
    .await
}

pub(crate) async fn pin_status_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(subject) = find_owned_local_status_response_subject(&db, &status_id, &viewer).await?
    else {
        return Response::error("status not found", 404);
    };

    pin_local_status(&db, viewer.id(), &subject.status.id).await?;
    enqueue_add_featured_status_activity(&db, &config, &viewer, &subject.status).await?;
    enqueue_outbox_process_queue_best_effort(&ctx.env, "status_pin").await;
    Response::from_json(&pinned_status_response(&db, &config, &viewer, subject).await?)
}

pub(crate) async fn unpin_status_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(subject) = find_owned_local_status_response_subject(&db, &status_id, &viewer).await?
    else {
        return Response::error("status not found", 404);
    };

    unpin_local_status(&db, viewer.id(), &subject.status.id).await?;
    enqueue_remove_featured_status_activity(&db, &config, &viewer, &subject.status).await?;
    enqueue_outbox_process_queue_best_effort(&ctx.env, "status_unpin").await;
    Response::from_json(&pinned_status_response(&db, &config, &viewer, subject).await?)
}
