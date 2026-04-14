use super::{
    MAX_AV_UPLOAD_BYTES, MAX_IMAGE_UPLOAD_BYTES, MediaKind, Request, UpdateMediaRequest,
    classify_media_kind, media_kind_label,
};
use worker::FormEntry;

#[derive(Debug)]
pub(crate) struct MediaUploadDraft {
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: String,
    pub(crate) description: String,
    pub(crate) kind: MediaKind,
}

pub(crate) async fn parse_media_upload(
    req: &mut Request,
) -> std::result::Result<MediaUploadDraft, String> {
    let form = req
        .form_data()
        .await
        .map_err(|error| format!("invalid multipart media payload: {error}"))?;

    let file = match form.get("file") {
        Some(FormEntry::File(file)) => file,
        Some(FormEntry::Field(_)) => {
            return Err("file field must be sent as multipart file data".to_owned());
        }
        None => return Err("file field is required".to_owned()),
    };

    let content_type = file.type_().trim().to_ascii_lowercase();
    if content_type.is_empty() {
        return Err("uploaded file is missing a content type".to_owned());
    }

    let kind = classify_media_kind(&content_type)
        .ok_or_else(|| format!("unsupported media content type: {content_type}"))?;
    let bytes = file
        .bytes()
        .await
        .map_err(|error| format!("failed to read uploaded file: {error}"))?;
    if bytes.is_empty() {
        return Err("uploaded file must not be empty".to_owned());
    }

    let size_limit = max_upload_size(kind);
    if bytes.len() > size_limit {
        return Err(format!(
            "uploaded file exceeds the {} byte limit for {} uploads",
            size_limit,
            media_kind_label(kind)
        ));
    }

    Ok(MediaUploadDraft {
        bytes,
        content_type,
        description: form
            .get_field("description")
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_default(),
        kind,
    })
}

pub(crate) async fn parse_media_update_request(
    req: &mut Request,
) -> std::result::Result<UpdateMediaRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let request = if content_type.contains("application/json") {
        req.json::<UpdateMediaRequest>()
            .await
            .map_err(|error| format!("invalid JSON media update payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form media update payload: {error}"))?;
        UpdateMediaRequest {
            description: form.get_field("description"),
            focus: form.get_field("focus"),
        }
    };

    Ok(request)
}

pub(crate) fn parse_media_focus(
    focus: Option<&str>,
) -> std::result::Result<Option<(f64, f64)>, String> {
    let Some(focus) = focus.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some((x, y)) = focus.split_once(',') else {
        return Err("focus must be in the form `x,y`".to_owned());
    };
    let x = x
        .trim()
        .parse::<f64>()
        .map_err(|_| "focus x must be a number".to_owned())?;
    let y = y
        .trim()
        .parse::<f64>()
        .map_err(|_| "focus y must be a number".to_owned())?;
    if !(-1.0..=1.0).contains(&x) || !(-1.0..=1.0).contains(&y) {
        return Err("focus coordinates must be between -1.0 and 1.0".to_owned());
    }
    Ok(Some((x, y)))
}

const fn max_upload_size(kind: MediaKind) -> usize {
    match kind {
        MediaKind::Image => MAX_IMAGE_UPLOAD_BYTES,
        MediaKind::Video | MediaKind::Audio => MAX_AV_UPLOAD_BYTES,
    }
}
