mod html_preview;
mod interaction_accounts;
mod object_routes;
mod preview_card;
mod request_context;

pub(crate) use html_preview::*;
pub(crate) use interaction_accounts::*;
pub(crate) use object_routes::*;
pub(crate) use preview_card::*;
pub(crate) use request_context::*;

use super::{
    CACHE_TTL_STATUS_API, Request, Response, Result, RouteContext,
    build_finished_context_async_refresh_header, build_local_status_context,
    build_local_status_response, build_remote_status_context, build_remote_status_response,
    cache_public_json_response, cache_status_api_response, cached_status_api_response,
    find_remote_actor_by_actor_uri, find_remote_status_attachments_by_status_id,
    find_remote_status_by_id, find_visible_local_status_response_subject,
    is_public_activitypub_visibility, list_remote_status_edit_snapshots,
    load_remote_status_updated_at, load_visible_local_status_response_subject,
    resolve_local_status_response_subject, timestamp_to_mastodon_iso8601,
};
use serde::Serialize;

enum LoadedStatusApiSubject {
    Local(super::LoadedLocalStatusResponseSubject),
    Remote {
        status: crate::RemoteStatusRow,
        actor: crate::RemoteActorRow,
    },
}

#[derive(Debug, Serialize)]
struct StatusSourceResponse {
    id: String,
    text: String,
    spoiler_text: String,
}

pub(crate) fn normalize_status_history_entry(mut value: serde_json::Value) -> serde_json::Value {
    let content = value
        .get("content")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(""));
    let spoiler_text = value
        .get("spoiler_text")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(""));
    let sensitive = value
        .get("sensitive")
        .cloned()
        .unwrap_or(serde_json::Value::Bool(false));
    let created_at = value
        .get("created_at")
        .and_then(serde_json::Value::as_str)
        .map(timestamp_to_mastodon_iso8601)
        .unwrap_or_default();
    let account = value
        .get("account")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let media_attachments = match value.get_mut("media_attachments") {
        Some(serde_json::Value::Array(items)) => serde_json::Value::Array(items.clone()),
        _ => serde_json::json!([]),
    };
    let emojis = match value.get_mut("emojis") {
        Some(serde_json::Value::Array(items)) => serde_json::Value::Array(items.clone()),
        _ => serde_json::json!([]),
    };
    let poll = value
        .get("poll")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let quote = value
        .get("quote")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    serde_json::json!({
        "content": content,
        "spoiler_text": spoiler_text,
        "sensitive": sensitive,
        "created_at": created_at,
        "account": account,
        "media_attachments": media_attachments,
        "emojis": emojis,
        "poll": poll,
        "quote": quote,
    })
}

pub(crate) async fn status_card_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(detail) = resolve_status_detail_base_context(&req, &ctx)? else {
        return Response::error("missing status id route parameter", 400);
    };

    let Some(status) =
        resolve_status_reference(&detail.db, &detail.config, &detail.status_id).await?
    else {
        return Response::error("status not found", 404);
    };

    let mut card = match status {
        ResolvedStatus::Local(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            build_status_card_value(&status.text)
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            let attachments =
                find_remote_status_attachments_by_status_id(&detail.db, &status.id).await?;
            build_remote_status_card_value(&status.plain_text(), &attachments)
        }
    }
    .unwrap_or(serde_json::Value::Null);
    let _ = enrich_card_with_remote_preview(&mut card).await;

    crate::with_d1_bookmark(Response::from_json(&card)?, &detail.session)
}

pub(crate) async fn status_api_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(detail) = resolve_status_detail_request_context(&req, &ctx).await? else {
        return Response::error("missing status id route parameter", 400);
    };
    if detail.viewer.is_none()
        && let Some(response) = cached_status_api_response(&ctx, &detail.base.status_id).await?
    {
        return Ok(response);
    }
    let Some(status) =
        resolve_status_reference(&detail.base.db, &detail.base.config, &detail.base.status_id)
            .await?
    else {
        return Response::error("status not found", 404);
    };

    let Some(subject) = load_status_api_subject(
        &detail.base.db,
        &detail.base.config,
        detail.viewer.as_ref(),
        status,
    )
    .await?
    else {
        return Response::error("status not found", 404);
    };
    let response = build_status_api_document(
        &detail.base.db,
        &detail.base.config,
        detail.viewer.as_ref(),
        subject,
    )
    .await?;
    if detail.viewer.is_none() {
        cache_status_api_response(&ctx, &detail.base.status_id, &response).await?;
        let cached = cache_public_json_response(
            &response,
            "application/json; charset=utf-8",
            CACHE_TTL_STATUS_API,
            &[("Cache-Tag", &format!("status-{}", detail.base.status_id))],
        )?;
        return crate::with_d1_bookmark(cached, &detail.base.session);
    }
    crate::with_d1_bookmark(Response::from_json(&response)?, &detail.base.session)
}

async fn load_status_api_subject(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: ResolvedStatus,
) -> Result<Option<LoadedStatusApiSubject>> {
    match status {
        ResolvedStatus::Local(status) => {
            load_local_status_api_subject(db, config, viewer, status).await
        }
        ResolvedStatus::Remote(status) => load_remote_status_api_subject(db, status).await,
    }
}

async fn build_status_api_document(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    subject: LoadedStatusApiSubject,
) -> Result<crate::MastodonStatusResponse> {
    match subject {
        LoadedStatusApiSubject::Local(subject) => {
            let super::LoadedLocalStatusResponseSubject {
                status,
                account,
                preload:
                    super::LocalStatusResponsePreload {
                        in_reply_to_account_id,
                        media,
                    },
            } = subject;
            build_local_status_response(
                db,
                config,
                viewer,
                &status,
                &account,
                in_reply_to_account_id,
                media,
            )
            .await
        }
        LoadedStatusApiSubject::Remote { status, actor } => {
            build_remote_status_response(db, config, viewer, &status, &actor).await
        }
    }
}

async fn load_local_status_api_subject(
    db: &crate::D1Database,
    _config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: crate::StatusRow,
) -> Result<Option<LoadedStatusApiSubject>> {
    match resolve_local_status_response_subject(db, viewer, status).await? {
        Some(super::ResolvedLocalStatusResponseSubject::Loaded(subject)) => {
            Ok(Some(LoadedStatusApiSubject::Local(subject)))
        }
        Some(super::ResolvedLocalStatusResponseSubject::Hidden) | None => Ok(None),
    }
}

async fn load_remote_status_api_subject(
    db: &crate::D1Database,
    status: crate::RemoteStatusRow,
) -> Result<Option<LoadedStatusApiSubject>> {
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Ok(None);
    }
    let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
        return Ok(None);
    };
    Ok(Some(LoadedStatusApiSubject::Remote { status, actor }))
}

async fn context_response_with_async_refresh<T: Serialize>(
    db: &crate::D1Database,
    status_id: &str,
    viewer_present: bool,
    context: &T,
) -> Result<Response> {
    let mut response = Response::from_json(context)?;
    if viewer_present {
        let header = build_finished_context_async_refresh_header(db, status_id).await?;
        response
            .headers_mut()
            .set("Mastodon-Async-Refresh", &header)?;
    }
    Ok(response)
}

fn status_history_response_from_parts(
    response: crate::MastodonStatusResponse,
    created_at: String,
    snapshots: Vec<serde_json::Value>,
) -> Result<Response> {
    let mut current_revision = serde_json::to_value(response).unwrap_or(serde_json::json!({}));
    current_revision["created_at"] = serde_json::json!(created_at);
    let mut history = vec![normalize_status_history_entry(current_revision)];
    history.extend(snapshots);
    Response::from_json(&history)
}

pub(crate) async fn status_source_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(detail) = resolve_status_detail_request_context(&req, &ctx).await? else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(viewer) = detail.viewer else {
        return Response::error("Auth0 authentication required", 401);
    };
    let Some(status) =
        resolve_status_reference(&detail.base.db, &detail.base.config, &detail.base.status_id)
            .await?
    else {
        return Response::error("status not found", 404);
    };
    let ResolvedStatus::Local(status) = status else {
        return Response::error("status source is only available for local statuses", 403);
    };
    let Some(subject) =
        load_visible_local_status_response_subject(&detail.base.db, Some(&viewer), status).await?
    else {
        return Response::error("status not found", 404);
    };
    let super::LoadedLocalStatusResponseSubject { status, .. } = subject;

    crate::with_d1_bookmark(
        Response::from_json(&StatusSourceResponse {
            id: status.id,
            text: status.text,
            spoiler_text: status.spoiler_text,
        })?,
        &detail.base.session,
    )
}

pub(crate) async fn status_context_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(detail) = resolve_status_detail_request_context(&req, &ctx).await? else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(status) =
        resolve_status_reference(&detail.base.db, &detail.base.config, &detail.base.status_id)
            .await?
    else {
        return Response::error("status not found", 404);
    };

    let response = match status {
        ResolvedStatus::Local(status) => {
            let Some(subject) = load_visible_local_status_response_subject(
                &detail.base.db,
                detail.viewer.as_ref(),
                status,
            )
            .await?
            else {
                return Response::error("status not found", 404);
            };
            let super::LoadedLocalStatusResponseSubject {
                status,
                account: owner,
                ..
            } = subject;

            let context = build_local_status_context(
                &detail.base.db,
                &detail.base.config,
                detail.viewer.as_ref(),
                &status,
                &owner,
            )
            .await?;
            context_response_with_async_refresh(
                &detail.base.db,
                &status.id,
                detail.viewer.is_some(),
                &context,
            )
            .await?
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            let Some(actor) =
                find_remote_actor_by_actor_uri(&detail.base.db, &status.actor_uri).await?
            else {
                return Response::error("status not found", 404);
            };
            let context = build_remote_status_context(
                &detail.base.db,
                &detail.base.config,
                detail.viewer.as_ref(),
                &status,
                &actor,
            )
            .await?;
            context_response_with_async_refresh(
                &detail.base.db,
                &status.id,
                detail.viewer.is_some(),
                &context,
            )
            .await?
        }
    };
    crate::with_d1_bookmark(response, &detail.base.session)
}

pub(crate) async fn status_history_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(detail) = resolve_status_detail_request_context(&req, &ctx).await? else {
        return Response::error("missing status id route parameter", 400);
    };

    if let Some(subject) = find_visible_local_status_response_subject(
        &detail.base.db,
        detail.viewer.as_ref(),
        &detail.base.status_id,
    )
    .await?
    {
        let super::LoadedLocalStatusResponseSubject {
            status,
            account,
            preload,
        } = subject;
        let response = build_local_status_response(
            &detail.base.db,
            &detail.base.config,
            detail.viewer.as_ref(),
            &status,
            &account,
            preload.in_reply_to_account_id,
            preload.media,
        )
        .await?;
        let created_at = crate::load_status_updated_at(&detail.base.db, &status.id)
            .await?
            .unwrap_or_else(|| status.created_at.clone());
        return crate::with_d1_bookmark(
            status_history_response_from_parts(
                response,
                created_at,
                crate::list_status_edit_snapshots(&detail.base.db, &status.id).await?,
            )?,
            &detail.base.session,
        );
    }

    if let Some(status) = find_remote_status_by_id(&detail.base.db, &detail.base.status_id).await? {
        if !is_public_activitypub_visibility(status.visibility.as_str()) {
            return Response::error("status not found", 404);
        }
        let Some(actor) =
            find_remote_actor_by_actor_uri(&detail.base.db, &status.actor_uri).await?
        else {
            return Response::error("status not found", 404);
        };

        let response = build_remote_status_response(
            &detail.base.db,
            &detail.base.config,
            detail.viewer.as_ref(),
            &status,
            &actor,
        )
        .await?;
        let created_at = load_remote_status_updated_at(&detail.base.db, &status.id)
            .await?
            .unwrap_or_else(|| status.published_at.clone());
        return crate::with_d1_bookmark(
            status_history_response_from_parts(
                response,
                created_at,
                list_remote_status_edit_snapshots(&detail.base.db, &status.id).await?,
            )?,
            &detail.base.session,
        );
    }

    Response::error("status not found", 404)
}
