use crate::media::UpdateMediaRequest;
use crate::runtime_config::{MAX_AV_UPLOAD_BYTES, MAX_IMAGE_UPLOAD_BYTES};
use crate::store::media::{MediaKind, MediaUploadDraft, classify_media_kind, media_kind_label};
use worker::{FormEntry, Request};

pub(crate) async fn parse_media_upload(
    req: &mut Request,
) -> std::result::Result<MediaUploadDraft, String> {
    // Kept for clients whose file part has no `filename`, which the runtime
    // turns into a lossy text field (see `multipart_file_part`).
    let mut raw_request = req
        .clone()
        .map_err(|error| format!("invalid multipart media payload: {error}"))?;
    let form = req
        .form_data()
        .await
        .map_err(|error| format!("invalid multipart media payload: {error}"))?;

    let (bytes, declared_type) = match form.get("file") {
        Some(FormEntry::File(file)) => (
            file.bytes()
                .await
                .map_err(|error| format!("failed to read uploaded file: {error}"))?,
            file.type_(),
        ),
        Some(FormEntry::Field(_)) => {
            let content_type = raw_request
                .headers()
                .get("Content-Type")
                .ok()
                .flatten()
                .unwrap_or_default();
            let body = raw_request
                .bytes()
                .await
                .map_err(|error| format!("failed to read uploaded file: {error}"))?;
            multipart_file_part(&content_type, &body, "file")
                .ok_or_else(|| "file field must be sent as multipart file data".to_owned())?
        }
        None => return Err("file field is required".to_owned()),
    };
    if bytes.is_empty() {
        return Err("uploaded file must not be empty".to_owned());
    }

    let declared_type = declared_type.trim().to_ascii_lowercase();
    let content_type = resolve_upload_content_type(&declared_type, &bytes);
    if content_type.is_empty() {
        return Err("uploaded file is missing a content type".to_owned());
    }
    let kind = classify_media_kind(&content_type)
        .ok_or_else(|| format!("unsupported media content type: {content_type}"))?;

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

/// Bytes and part Content-Type of the multipart part named `field`.
///
/// The Workers runtime only yields a `File` when the part carries a `filename`;
/// some clients omit it, and Rack (Mastodon) still accepts such parts as uploads.
pub(crate) fn multipart_file_part(
    content_type: &str,
    body: &[u8],
    field: &str,
) -> Option<(Vec<u8>, String)> {
    let boundary = content_type.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| value.trim().trim_matches('"').to_owned())
    })?;
    let delimiter = format!("--{boundary}");
    let mut rest = body;
    loop {
        let start = find_bytes(rest, delimiter.as_bytes())? + delimiter.len();
        rest = &rest[start..];
        if rest.starts_with(b"--") {
            return None;
        }
        let headers_end = find_bytes(rest, b"\r\n\r\n")?;
        let headers = String::from_utf8_lossy(&rest[..headers_end]);
        let content = &rest[headers_end + 4..];
        let content_end = find_bytes(content, format!("\r\n{delimiter}").as_bytes())?;
        if multipart_part_name(&headers).as_deref() == Some(field) {
            let part_type = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-type")
                        .then(|| value.trim().to_owned())
                })
                .unwrap_or_default();
            return Some((content[..content_end].to_vec(), part_type));
        }
        rest = &content[content_end + 2..];
    }
}

fn multipart_part_name(headers: &str) -> Option<String> {
    let disposition = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-disposition")
            .then_some(value)
    })?;
    disposition.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        (name.trim() == "name").then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Mastodon trusts file contents over the multipart part's declared type, so
/// clients that send `application/octet-stream` (or nothing) still upload.
/// A declared media type is kept; otherwise the magic bytes decide.
pub(crate) fn resolve_upload_content_type(declared_type: &str, bytes: &[u8]) -> String {
    let declared_type = declared_type.split(';').next().unwrap_or_default().trim();
    if classify_media_kind(declared_type).is_some() {
        return declared_type.to_owned();
    }
    sniff_media_content_type(bytes).map_or_else(|| declared_type.to_owned(), str::to_owned)
}

fn sniff_media_content_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(b"\xFF\xD8\xFF") {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") {
        return match &bytes[8..12] {
            b"WEBP" => Some("image/webp"),
            b"WAVE" => Some("audio/wav"),
            _ => None,
        };
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some(match &bytes[8..12] {
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => "image/heic",
            b"avif" | b"avis" => "image/avif",
            b"qt  " => "video/quicktime",
            b"M4A " => "audio/mp4",
            _ => "video/mp4",
        });
    }
    if bytes.starts_with(b"\x1A\x45\xDF\xA3") {
        return Some("video/webm");
    }
    if bytes.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if bytes.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
    if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0)
    {
        return Some("audio/mpeg");
    }
    None
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
