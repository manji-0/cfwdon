use crate::request_utils::{
    build_internal_cursor_link_for_url, build_internal_cursor_link_for_url_with_min_id,
    parse_internal_pagination_id, parse_media_id_fields,
};
use url::Url;
use worker::FormEntry;

#[test]
fn parse_internal_pagination_id_accepts_integer_cursor() {
    assert_eq!(
        parse_internal_pagination_id(Some("42"), "max_id").unwrap(),
        Some(42)
    );
    assert_eq!(
        parse_internal_pagination_id(Some(""), "max_id").unwrap(),
        None
    );
    assert_eq!(parse_internal_pagination_id(None, "max_id").unwrap(), None);
}

#[test]
fn parse_internal_pagination_id_rejects_invalid_cursor() {
    let error = parse_internal_pagination_id(Some("abc"), "since_id").unwrap_err();
    assert!(error.to_string().contains("since_id"));
}

#[test]
fn internal_cursor_link_header_preserves_other_query_params() {
    let url = Url::parse("https://social.example/api/v1/mutes?foo=bar&limit=20").unwrap();
    let next = build_internal_cursor_link_for_url(&url, 10, Some(150), None, "next").unwrap();
    let prev = build_internal_cursor_link_for_url(&url, 10, None, Some(200), "prev").unwrap();

    assert!(next.contains("foo=bar"));
    assert!(next.contains("limit=10"));
    assert!(next.contains("max_id=150"));
    assert!(next.contains("rel=\"next\""));
    assert!(prev.contains("foo=bar"));
    assert!(prev.contains("limit=10"));
    assert!(prev.contains("since_id=200"));
    assert!(prev.contains("rel=\"prev\""));
}

#[test]
fn internal_cursor_link_header_supports_min_id_cursor() {
    let url =
        Url::parse("https://social.example/api/v1/scheduled_statuses?foo=bar&max_id=5").unwrap();
    let prev =
        build_internal_cursor_link_for_url_with_min_id(&url, 10, None, None, Some(200), "prev")
            .unwrap();

    assert!(prev.contains("foo=bar"));
    assert!(prev.contains("limit=10"));
    assert!(prev.contains("min_id=200"));
    assert!(!prev.contains("max_id=5"));
    assert!(prev.contains("rel=\"prev\""));
}

#[test]
fn parse_media_id_fields_accepts_bracketed_and_plain_form_keys() {
    assert_eq!(
        parse_media_id_fields([
            Some(vec![FormEntry::Field("media-1".to_owned())]),
            Some(vec![FormEntry::Field("media-2".to_owned())]),
            Some(vec![FormEntry::Field("media-indexed".to_owned())]),
        ]),
        Some(vec![
            "media-1".to_owned(),
            "media-2".to_owned(),
            "media-indexed".to_owned()
        ])
    );
    assert_eq!(
        parse_media_id_fields([None, Some(vec![FormEntry::Field("media-plain".to_owned())]),]),
        Some(vec!["media-plain".to_owned()])
    );
    assert_eq!(parse_media_id_fields([None, None]), None);
}
