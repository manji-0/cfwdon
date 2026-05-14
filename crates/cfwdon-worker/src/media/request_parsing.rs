use super::{MediaKind, classify_media_kind, media_kind_label};
use crate::{MAX_AV_UPLOAD_BYTES, MAX_IMAGE_UPLOAD_BYTES, Request, UpdateMediaRequest};
use worker::FormEntry;

#[derive(Debug)]
pub(crate) struct MediaUploadDraft {
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: String,
    pub(crate) description: String,
    pub(crate) kind: MediaKind,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
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

    let dimensions = image_dimensions(&content_type, &bytes);

    Ok(MediaUploadDraft {
        width: dimensions.map(|(width, _)| width),
        height: dimensions.map(|(_, height)| height),
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

pub(crate) fn image_dimensions(content_type: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    match content_type {
        "image/png" => png_dimensions(bytes),
        "image/jpeg" | "image/jpg" => jpeg_dimensions(bytes),
        "image/gif" => gif_dimensions(bytes),
        "image/webp" => webp_dimensions(bytes),
        _ => None,
    }
    .filter(|(width, height)| *width > 0 && *height > 0)
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    ))
}

fn gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 || !matches!(&bytes[..6], b"GIF87a" | b"GIF89a") {
        return None;
    }
    Some((
        u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
        u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
    ))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }

    let mut offset = 2;
    while offset + 3 < bytes.len() {
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        if offset >= bytes.len() {
            return None;
        }

        let marker = bytes[offset];
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if offset + 2 > bytes.len() {
            return None;
        }
        let segment_len = u16::from_be_bytes(bytes[offset..offset + 2].try_into().ok()?) as usize;
        if segment_len < 2 || offset + segment_len > bytes.len() {
            return None;
        }

        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if segment_len < 7 {
                return None;
            }
            let height = u16::from_be_bytes(bytes[offset + 3..offset + 5].try_into().ok()?);
            let width = u16::from_be_bytes(bytes[offset + 5..offset + 7].try_into().ok()?);
            return Some((width as u32, height as u32));
        }

        offset += segment_len;
    }

    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }

    let chunk = &bytes[12..16];
    let data = &bytes[20..];
    match chunk {
        b"VP8X" if data.len() >= 10 => {
            let width =
                1 + u32::from(data[4]) + (u32::from(data[5]) << 8) + (u32::from(data[6]) << 16);
            let height =
                1 + u32::from(data[7]) + (u32::from(data[8]) << 8) + (u32::from(data[9]) << 16);
            Some((width, height))
        }
        b"VP8L" if data.len() >= 5 && data[0] == 0x2f => {
            let width = 1 + u32::from(data[1]) + ((u32::from(data[2]) & 0x3f) << 8);
            let height = 1
                + ((u32::from(data[2]) & 0xc0) >> 6)
                + (u32::from(data[3]) << 2)
                + ((u32::from(data[4]) & 0x0f) << 10);
            Some((width, height))
        }
        b"VP8 " if data.len() >= 10 && data[3..6] == [0x9d, 0x01, 0x2a] => {
            let width = u16::from_le_bytes(data[6..8].try_into().ok()?) & 0x3fff;
            let height = u16::from_le_bytes(data[8..10].try_into().ok()?) & 0x3fff;
            Some((width as u32, height as u32))
        }
        _ => None,
    }
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
