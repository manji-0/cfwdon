use crate::store::media::{classify_media_kind, media_kind_label};

#[test]
fn classify_media_kind_detects_supported_types() {
    assert_eq!(
        classify_media_kind("image/png").map(media_kind_label),
        Some("image")
    );
    assert_eq!(
        classify_media_kind("video/mp4").map(media_kind_label),
        Some("video")
    );
    assert_eq!(
        classify_media_kind("audio/ogg").map(media_kind_label),
        Some("audio")
    );
    assert_eq!(classify_media_kind("application/pdf"), None);
}
