use super::store::{
    create_custom_emoji, delete_custom_emoji, list_admin_custom_emojis,
    normalize_custom_emoji_shortcode, normalize_custom_emoji_upload, shortcode_taken,
    update_custom_emoji,
};
use crate::{
    Response, Result, RouteContext, is_admin_account, load_config, parse_optional_bool,
    require_authenticated_local_account, resolve_custom_emojis,
};
use serde::Deserialize;
use worker::{FormEntry, Request};

#[derive(Debug, Default, Deserialize)]
struct UpdateCustomEmojiRequest {
    visible_in_picker: Option<bool>,
    category: Option<String>,
}

pub(crate) async fn admin_custom_emojis_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) if is_admin_account(&config, &account) => {}
        Some(_) => return Response::error("Forbidden", 403),
        None => return Response::error("Auth0 authentication required", 401),
    }
    let emojis = list_admin_custom_emojis(&db, &config).await?;
    Response::from_json(&emojis)
}

pub(crate) async fn admin_create_custom_emoji_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) if is_admin_account(&config, &account) => {}
        Some(_) => return Response::error("Forbidden", 403),
        None => return Response::error("Auth0 authentication required", 401),
    }

    let form = req
        .form_data()
        .await
        .map_err(|error| worker::Error::RustError(format!("invalid multipart payload: {error}")))?;
    let shortcode = normalize_custom_emoji_shortcode(form.get_field("shortcode"))?;
    if shortcode_taken(&db, &shortcode).await? {
        return Response::error("shortcode has already been taken", 422);
    }
    let resolved = resolve_custom_emojis(&db, &config).await?;
    if resolved.iter().any(|emoji| emoji.shortcode == shortcode) {
        return Response::error("shortcode has already been taken", 422);
    }

    let file = match form.get("image") {
        Some(FormEntry::File(file)) => file,
        Some(FormEntry::Field(_)) => {
            return Response::error("image field must be sent as multipart file data", 422);
        }
        None => return Response::error("image field is required", 422),
    };
    let content_type = file.type_().trim().to_ascii_lowercase();
    let bytes = file.bytes().await.map_err(|error| {
        worker::Error::RustError(format!("failed to read uploaded image: {error}"))
    })?;
    let static_image = read_optional_static_image(&form).await?;
    let visible_in_picker =
        parse_optional_bool(form.get_field("visible_in_picker").as_deref())?.unwrap_or(true);
    let draft = normalize_custom_emoji_upload(
        shortcode,
        bytes,
        content_type,
        visible_in_picker,
        form.get_field("category"),
        static_image,
    )
    .map_err(worker::Error::RustError)?;

    let created = create_custom_emoji(&db, &bucket, &config, draft).await?;
    Response::from_json(&created)
}

pub(crate) async fn admin_update_custom_emoji_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let emoji_id = ctx
        .param("id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing custom emoji id route parameter".to_owned())
        })?
        .to_owned();
    let db = crate::bind_request_d1(&ctx, &config)?;
    let bucket = ctx.bucket(&config.media_binding).ok();
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) if is_admin_account(&config, &account) => {}
        Some(_) => return Response::error("Forbidden", 403),
        None => return Response::error("Auth0 authentication required", 401),
    }

    let content_type = req
        .headers()
        .get("Content-Type")
        .ok()
        .flatten()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    let (request, image) = if content_type.contains("application/json") {
        let request: UpdateCustomEmojiRequest = req
            .json()
            .await
            .map_err(|error| worker::Error::RustError(format!("invalid JSON payload: {error}")))?;
        (request, None)
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| worker::Error::RustError(format!("invalid form payload: {error}")))?;
        let request = UpdateCustomEmojiRequest {
            visible_in_picker: parse_optional_bool(form.get_field("visible_in_picker").as_deref())?,
            category: form.get_field("category"),
        };
        let image = read_optional_image_upload(&form).await?;
        (request, image)
    };

    let category = if request.category.is_some() {
        Some(
            request
                .category
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
        )
    } else {
        None
    };

    let updated = update_custom_emoji(
        &db,
        bucket.as_ref(),
        &config,
        &emoji_id,
        request.visible_in_picker,
        category,
        image,
    )
    .await?;
    match updated {
        Some(value) => Response::from_json(&value),
        None => Response::error("custom emoji not found", 404),
    }
}

pub(crate) async fn admin_delete_custom_emoji_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let emoji_id = ctx
        .param("id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing custom emoji id route parameter".to_owned())
        })?
        .to_owned();
    let db = crate::bind_request_d1(&ctx, &config)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) if is_admin_account(&config, &account) => {}
        Some(_) => return Response::error("Forbidden", 403),
        None => return Response::error("Auth0 authentication required", 401),
    }
    let _ = req;

    if delete_custom_emoji(&db, &bucket, &emoji_id).await? {
        Response::empty().map(|response| response.with_status(200))
    } else {
        Response::error("custom emoji not found", 404)
    }
}

async fn read_optional_image_upload(
    form: &worker::FormData,
) -> Result<Option<super::store::CustomEmojiUploadDraft>> {
    let Some(entry) = form.get("image") else {
        return Ok(None);
    };
    let file = match entry {
        FormEntry::File(file) => file,
        FormEntry::Field(_) => {
            return Err(worker::Error::RustError(
                "image field must be sent as multipart file data".to_owned(),
            ));
        }
    };
    let content_type = file.type_().trim().to_ascii_lowercase();
    let bytes = file.bytes().await.map_err(|error| {
        worker::Error::RustError(format!("failed to read uploaded image: {error}"))
    })?;
    let static_image = read_optional_static_image(form).await?;
    let draft =
        normalize_custom_emoji_upload(String::new(), bytes, content_type, true, None, static_image)
            .map_err(worker::Error::RustError)?;
    Ok(Some(draft))
}

async fn read_optional_static_image(form: &worker::FormData) -> Result<Option<(Vec<u8>, String)>> {
    let Some(entry) = form.get("static_image") else {
        return Ok(None);
    };
    let file = match entry {
        FormEntry::File(file) => file,
        FormEntry::Field(_) => {
            return Err(worker::Error::RustError(
                "static_image field must be sent as multipart file data".to_owned(),
            ));
        }
    };
    let content_type = file.type_().trim().to_ascii_lowercase();
    let bytes = file.bytes().await.map_err(|error| {
        worker::Error::RustError(format!("failed to read uploaded static_image: {error}"))
    })?;
    Ok(Some((bytes, content_type)))
}
