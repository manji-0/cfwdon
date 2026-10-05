use crate::media::{
    RemoteStatusAttachmentRow, image_dimensions, multipart_file_part, parse_media_focus,
    resolve_upload_content_type,
};
use crate::statuses::build_remote_status_card_value;

#[test]
fn parse_media_focus_accepts_valid_coordinates() {
    assert_eq!(
        parse_media_focus(Some("0.25,-0.5")).unwrap(),
        Some((0.25, -0.5))
    );
    assert_eq!(parse_media_focus(Some("")).unwrap(), None);
    assert_eq!(parse_media_focus(None).unwrap(), None);
}

#[test]
fn parse_media_focus_rejects_invalid_coordinates() {
    assert!(parse_media_focus(Some("1.5,0")).is_err());
    assert!(parse_media_focus(Some("abc,0")).is_err());
    assert!(parse_media_focus(Some("0")).is_err());
}

#[test]
fn image_dimensions_reads_common_image_headers() {
    let mut png = Vec::from(&b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"[..]);
    png.extend_from_slice(&640u32.to_be_bytes());
    png.extend_from_slice(&480u32.to_be_bytes());

    let jpeg = [
        0xff, 0xd8, 0xff, 0xc0, 0x00, 0x07, 0x08, 0x03, 0x20, 0x04, 0x00,
    ];
    let gif = b"GIF89a\x20\x03\x58\x02";

    assert_eq!(image_dimensions("image/png", &png), Some((640, 480)));
    assert_eq!(image_dimensions("image/jpeg", &jpeg), Some((1024, 800)));
    assert_eq!(image_dimensions("image/gif", gif), Some((800, 600)));
    assert_eq!(image_dimensions("image/png", b"not a png"), None);
}

#[test]
fn build_remote_status_card_value_prefers_link_attachment_metadata() {
    let attachments = vec![RemoteStatusAttachmentRow {
        id: "att-1".to_owned(),
        status_id: "status-1".to_owned(),
        remote_url: "https://news.example/articles/hello-world".to_owned(),
        preview_url: Some("https://cdn.example/preview.png".to_owned()),
        content_type: "text/html".to_owned(),
        description: Some("Hello World Article".to_owned()),
        blurhash: Some("LKO2?U%2Tw=w]~RBVZRi};RPxuwH".to_owned()),
        width: Some(1200),
        height: Some(630),
        created_at: "2026-01-01T00:00:00Z".to_owned(),
    }];

    let card =
        build_remote_status_card_value("context https://fallback.example/post", &attachments)
            .unwrap();

    assert_eq!(card["url"], "https://news.example/articles/hello-world");
    assert_eq!(card["provider_name"], "news.example");
    assert_eq!(card["provider_url"], "https://news.example");
    assert_eq!(card["title"], "Hello World Article");
    assert_eq!(card["description"], "context");
    assert_eq!(card["image"], "https://cdn.example/preview.png");
    assert_eq!(card["width"], 1200);
    assert_eq!(card["height"], 630);
    assert_eq!(card["blurhash"], "LKO2?U%2Tw=w]~RBVZRi};RPxuwH");
}

#[test]
fn build_remote_status_card_value_falls_back_without_link_attachment() {
    let attachments = vec![RemoteStatusAttachmentRow {
        id: "att-1".to_owned(),
        status_id: "status-1".to_owned(),
        remote_url: "https://cdn.example/image.png".to_owned(),
        preview_url: None,
        content_type: "image/png".to_owned(),
        description: Some("alt".to_owned()),
        blurhash: None,
        width: None,
        height: None,
        created_at: "2026-01-01T00:00:00Z".to_owned(),
    }];

    let card =
        build_remote_status_card_value("see https://example.com/post", &attachments).unwrap();

    assert_eq!(card["url"], "https://example.com/post");
    assert_eq!(card["provider_name"], "example.com");
    assert!(card["image"].is_null());
}

#[test]
fn resolve_upload_content_type_keeps_declared_media_types() {
    assert_eq!(
        resolve_upload_content_type("image/png", b"\xFF\xD8\xFFjpeg"),
        "image/png"
    );
    assert_eq!(
        resolve_upload_content_type("image/jpeg; name=a.jpg", b"\xFF\xD8\xFF"),
        "image/jpeg"
    );
}

#[test]
fn resolve_upload_content_type_sniffs_generic_or_missing_types() {
    let cases: [(&[u8], &str); 12] = [
        (b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", "image/png"),
        (b"\xFF\xD8\xFF\xE0\0\x10JFIF", "image/jpeg"),
        (b"GIF89a\x01\0\x01\0", "image/gif"),
        (b"RIFF\0\0\0\0WEBPVP8 ", "image/webp"),
        (b"\0\0\0\x18ftypheic\0\0\0\0", "image/heic"),
        (b"\0\0\0\x1cftypavif\0\0\0\0", "image/avif"),
        (b"\0\0\0\x18ftypisom\0\0\x02\0", "video/mp4"),
        (b"\0\0\0\x14ftypqt  \0\0\0\0", "video/quicktime"),
        (b"\x1A\x45\xDF\xA3\x01\0", "video/webm"),
        (b"OggS\0\x02", "audio/ogg"),
        (b"ID3\x04\0\0", "audio/mpeg"),
        (b"RIFF\0\0\0\0WAVEfmt ", "audio/wav"),
    ];
    for (bytes, expected) in cases {
        assert_eq!(
            resolve_upload_content_type("application/octet-stream", bytes),
            expected
        );
        assert_eq!(resolve_upload_content_type("", bytes), expected);
    }
}

#[test]
fn resolve_upload_content_type_leaves_unknown_bytes_unsupported() {
    assert_eq!(
        resolve_upload_content_type("application/octet-stream", b"%PDF-1.7"),
        "application/octet-stream"
    );
    assert_eq!(resolve_upload_content_type("", b"plain text"), "");
}

#[test]
fn multipart_file_part_reads_file_parts_without_filename() {
    let body = b"---b-1\r\nContent-Disposition: form-data; name=\"description\"\r\n\r\nalt\r\n\
---b-1\r\nContent-Disposition: form-data; name=\"file\"\r\nContent-Type: image/png\r\n\r\n\
\x89PNG\r\n\x1a\n\xff\x00\r\n---b-1--\r\n";
    let (bytes, content_type) =
        multipart_file_part("multipart/form-data; boundary=-b-1", body, "file").unwrap();
    assert_eq!(bytes, b"\x89PNG\r\n\x1a\n\xff\x00");
    assert_eq!(content_type, "image/png");
    assert_eq!(
        multipart_file_part("multipart/form-data; boundary=\"-b-1\"", body, "file").map(|p| p.0),
        Some(bytes)
    );
    assert!(multipart_file_part("multipart/form-data; boundary=-b-1", body, "missing").is_none());
    assert!(multipart_file_part("multipart/form-data", body, "file").is_none());
}
