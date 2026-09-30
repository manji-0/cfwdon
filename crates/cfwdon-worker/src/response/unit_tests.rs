use crate::response::{
    AUTH_CONTEXT_LIMIT, MastodonMediaAttachmentResponse, mastodon_account_fields,
    media_fallback_url, media_object_url, trim_context_ancestors, trim_context_descendants,
};
use crate::store::media::MediaAttachmentRow;
use cfwdon_core::AppConfig;
use cfwdon_domain::ProfileField;

#[test]
fn media_urls_prefer_custom_domain_and_keep_worker_fallback() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.media_public_base_url = Some("https://media.example.com".to_owned());
    assert_eq!(
        media_object_url(&config, "media/account/image/abc"),
        "https://media.example.com/media/account/image/abc"
    );
    assert_eq!(
        media_fallback_url(&config, "abc"),
        "https://social.example/media/abc"
    );
}

#[test]
fn local_media_response_uses_worker_route_without_public_media_base() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let response = MastodonMediaAttachmentResponse::from_row(
        &MediaAttachmentRow {
            id: "media-1".to_owned(),
            account_id: "acct-1".to_owned(),
            status_id: Some("status-1".to_owned()),
            object_key: "media/acct-1/image/media-1".to_owned(),
            content_type: "image/png".to_owned(),
            description: String::new(),
            focus_x: None,
            focus_y: None,
            width: Some(640),
            height: Some(480),
            _created_at: "2026-05-09 00:00:00".to_owned(),
        },
        &config,
    );
    let document = serde_json::to_value(response).expect("media response serializes");

    assert_eq!(
        document.pointer("/url"),
        Some(&serde_json::json!("https://social.example/media/media-1"))
    );
    assert_eq!(
        document.pointer("/preview_url"),
        Some(&serde_json::json!("https://social.example/media/media-1"))
    );
    assert_eq!(document.pointer("/text_url"), None);
    assert_eq!(
        document.pointer("/meta/original/width"),
        Some(&serde_json::json!(640))
    );
    assert_eq!(
        document.pointer("/meta/original/height"),
        Some(&serde_json::json!(480))
    );
    assert_eq!(
        document.pointer("/meta/original/aspect"),
        Some(&serde_json::json!(640.0 / 480.0))
    );
}

#[test]
fn local_media_response_uses_public_media_base_when_configured() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.media_public_base_url = Some("https://media.example.com".to_owned());
    let response = MastodonMediaAttachmentResponse::from_row(
        &MediaAttachmentRow {
            id: "media-1".to_owned(),
            account_id: "acct-1".to_owned(),
            status_id: Some("status-1".to_owned()),
            object_key: "media/acct-1/image/media-1".to_owned(),
            content_type: "image/png".to_owned(),
            description: String::new(),
            focus_x: None,
            focus_y: None,
            width: Some(640),
            height: Some(480),
            _created_at: "2026-05-09 00:00:00".to_owned(),
        },
        &config,
    );
    let document = serde_json::to_value(response).expect("media response serializes");

    assert_eq!(
        document.pointer("/url"),
        Some(&serde_json::json!(
            "https://media.example.com/media/acct-1/image/media-1"
        ))
    );
    assert_eq!(
        document.pointer("/preview_url"),
        Some(&serde_json::json!(
            "https://media.example.com/media/acct-1/image/media-1"
        ))
    );
    assert_eq!(document.pointer("/text_url"), None);
}

#[test]
fn local_media_response_keeps_meta_objects_when_dimensions_unknown() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let response = MastodonMediaAttachmentResponse::from_row(
        &MediaAttachmentRow {
            id: "media-1".to_owned(),
            account_id: "acct-1".to_owned(),
            status_id: Some("status-1".to_owned()),
            object_key: "media/acct-1/image/media-1".to_owned(),
            content_type: "image/png".to_owned(),
            description: String::new(),
            focus_x: None,
            focus_y: None,
            width: None,
            height: None,
            _created_at: "2026-05-09 00:00:00".to_owned(),
        },
        &config,
    );
    let document = serde_json::to_value(response).expect("media response serializes");

    assert!(document.pointer("/meta/original").is_some());
    assert!(document.pointer("/meta/small").is_some());
    assert!(document.pointer("/meta/original").unwrap().is_object());
    assert!(document.pointer("/meta/small").unwrap().is_object());
}

#[test]
fn mastodon_account_fields_render_urls_as_links() {
    let fields = vec![ProfileField {
        name: "Website".to_owned(),
        value: "https://example.com".to_owned(),
    }];
    let rendered = mastodon_account_fields(&fields);
    assert_eq!(rendered[0]["name"], serde_json::json!("Website"));
    assert!(
        rendered[0]["value"]
            .as_str()
            .unwrap_or_default()
            .contains("<a href=\"https://example.com\"")
    );
}

#[test]
fn trim_context_ancestors_keeps_nearest_unauthenticated_entries() {
    let ancestors = (0..50)
        .map(|index| format!("ancestor-{index}"))
        .collect::<Vec<_>>();

    let trimmed = trim_context_ancestors(ancestors, false);

    assert_eq!(trimmed.len(), 40);
    assert_eq!(trimmed.first().map(String::as_str), Some("ancestor-10"));
    assert_eq!(trimmed.last().map(String::as_str), Some("ancestor-49"));
}

#[test]
fn trim_context_descendants_limits_unauthenticated_entries() {
    let descendants = (0..80)
        .map(|index| format!("descendant-{index}"))
        .collect::<Vec<_>>();

    let trimmed = trim_context_descendants(descendants, false);

    assert_eq!(trimmed.len(), 60);
    assert_eq!(trimmed.first().map(String::as_str), Some("descendant-0"));
    assert_eq!(trimmed.last().map(String::as_str), Some("descendant-59"));
}

#[test]
fn trim_context_ancestors_limits_authenticated_entries() {
    let ancestors = (0..5000)
        .map(|index| format!("ancestor-{index}"))
        .collect::<Vec<_>>();

    let trimmed = trim_context_ancestors(ancestors, true);

    assert_eq!(trimmed.len(), AUTH_CONTEXT_LIMIT);
    assert_eq!(trimmed.first().map(String::as_str), Some("ancestor-904"));
    assert_eq!(trimmed.last().map(String::as_str), Some("ancestor-4999"));
}

#[test]
fn trim_context_descendants_limits_authenticated_entries() {
    let descendants = (0..5000)
        .map(|index| format!("descendant-{index}"))
        .collect::<Vec<_>>();

    let trimmed = trim_context_descendants(descendants, true);

    assert_eq!(trimmed.len(), AUTH_CONTEXT_LIMIT);
    assert_eq!(trimmed.first().map(String::as_str), Some("descendant-0"));
    assert_eq!(trimmed.last().map(String::as_str), Some("descendant-4095"));
}
