mod attachment_store;
mod request_parsing;
mod storage;

pub(crate) use attachment_store::*;
pub(crate) use request_parsing::*;
pub(crate) use storage::*;

use super::{
    Error, MastodonMediaAttachmentResponse, Request, Response, Result, RouteContext, load_config,
    media_object_url, observability_started_at_ms, require_authenticated_local_account,
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaKind {
    Image,
    Video,
    Audio,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct MediaAttachmentRow {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) status_id: Option<String>,
    pub(crate) object_key: String,
    pub(crate) content_type: String,
    pub(crate) description: String,
    pub(crate) focus_x: Option<f64>,
    pub(crate) focus_y: Option<f64>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    #[serde(rename = "created_at")]
    pub(crate) _created_at: String,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct OrphanMediaPruneResponse {
    pub(crate) deleted: u32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateMediaRequest {
    pub(crate) description: Option<String>,
    pub(crate) focus: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OrphanMediaRow {
    pub(crate) id: String,
    pub(crate) object_key: String,
}

pub(crate) async fn prune_orphan_media(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(_) => {}
        None => return Response::error("Auth0 authentication required", 401),
    }

    let bucket = ctx.bucket(&config.media_binding)?;
    let queued_deleted = delete_queued_media(&db, &bucket, 128).await?;
    let orphans = list_orphan_media(&db, 24, 128).await?;
    let deleted = queued_deleted + delete_orphan_media(&db, &bucket, &orphans).await?;

    Response::from_json(&OrphanMediaPruneResponse { deleted })
}

pub(crate) async fn create_media_attachment(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let draft = match parse_media_upload(&mut req).await {
        Ok(draft) => draft,
        Err(message) => return Response::error(message, 422),
    };

    let db = ctx.d1(&config.database_binding)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let media = store_media_attachment(&db, &bucket, &account, &draft).await?;

    Response::from_json(&MastodonMediaAttachmentResponse::from_row(&media, &config))
}

pub(crate) async fn media_content_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let media_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing media id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(media) = find_media_attachment_by_id(&db, &media_id).await? else {
        return Response::error("media not found", 404);
    };

    if config.media_public_base_url.is_some() {
        return Response::redirect(
            Url::parse(&media_object_url(&config, &media.object_key))
                .map_err(|error| Error::RustError(format!("invalid media public url: {error}")))?,
        );
    }

    let bucket = ctx.bucket(&config.media_binding)?;
    let r2_started_at_ms = observability_started_at_ms();
    let object_result = bucket.get(&media.object_key).execute().await;
    let object = match object_result {
        Ok(Some(object)) => {
            log_r2_operation("get", "hit", r2_started_at_ms, &media.object_key, None);
            object
        }
        Ok(None) => {
            log_r2_operation("get", "miss", r2_started_at_ms, &media.object_key, None);
            return Response::error("media object not found", 404);
        }
        Err(error) => {
            log_r2_operation("get", "error", r2_started_at_ms, &media.object_key, None);
            return Err(error);
        }
    };
    let Some(body) = object.body() else {
        return Response::error("media object body missing", 500);
    };

    let mut response = Response::from_body(body.response_body()?)?;
    response
        .headers_mut()
        .set("Content-Type", &media.content_type)?;
    response.headers_mut().set("ETag", &object.http_etag())?;
    response
        .headers_mut()
        .set("Cache-Control", "public, max-age=31536000, immutable")?;

    Ok(response)
}

pub(crate) async fn media_metadata_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let media_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing media id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(media) = find_media_attachment_by_id(&db, &media_id).await? else {
        return Response::error("media not found", 404);
    };

    Response::from_json(&MastodonMediaAttachmentResponse::from_row(&media, &config))
}

pub(crate) async fn update_media_attachment(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let media_id = match ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        Some(media_id) => media_id,
        None => return Response::error("missing media id route parameter", 400),
    };
    let update = match parse_media_update_request(&mut req).await {
        Ok(update) => update,
        Err(message) => return Response::error(message, 422),
    };

    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let media = match find_media_attachment_by_id(&db, &media_id).await? {
        Some(media) if media.account_id == account.id() => media,
        _ => return Response::error("media not found", 404),
    };

    let media = apply_media_update(&db, &media, update).await?;
    Response::from_json(&MastodonMediaAttachmentResponse::from_row(&media, &config))
}

pub(crate) async fn delete_media_attachment(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let media_id = match ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        Some(media_id) => media_id,
        None => return Response::error("missing media id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let media = match find_media_attachment_by_id(&db, &media_id).await? {
        Some(media) if media.account_id == account.id() => media,
        _ => return Response::error("media not found", 404),
    };
    if media.status_id.is_some() {
        return Response::error("media is already attached", 422);
    }

    let bucket = ctx.bucket(&config.media_binding)?;
    crate::delete_r2_object(&bucket, &media.object_key, "delete").await?;
    delete_media_attachment_row(&db, &media.id).await?;
    Response::from_json(&serde_json::json!({}))
}
