use crate::request_utils::{
    InternalPaginationIds, build_internal_cursor_link_for_url,
    build_internal_cursor_link_for_url_with_min_id, parse_media_id_fields,
};
use url::Url;
use worker::FormEntry;

fn max_id(value: &str) -> Option<Option<i64>> {
    InternalPaginationIds::parse(Some(value), None, None).map(|ids| ids.max_id)
}

#[test]
fn internal_pagination_ids_accept_integer_cursors() {
    assert_eq!(max_id("42"), Some(Some(42)));
    assert_eq!(max_id(" -7 "), Some(Some(-7)));
    assert_eq!(max_id(""), Some(None));
    assert_eq!(max_id("  "), Some(None));
    assert_eq!(
        InternalPaginationIds::parse(None, Some("3"), Some("5")),
        Some(InternalPaginationIds {
            max_id: None,
            since_id: Some(3),
            min_id: Some(5),
        })
    );
}

#[test]
fn internal_pagination_ids_cast_like_rails_integers() {
    assert_eq!(max_id("42abc"), Some(Some(42)));
    assert_eq!(max_id("+8"), Some(Some(8)));
    assert_eq!(max_id("abc"), None);
    assert_eq!(
        max_id("r_aHR0cHM6Ly9mZWRpYmlyZC5jb20vdXNlcnMvbWFuamkw"),
        None
    );
    assert_eq!(max_id("--1"), None);
    assert_eq!(max_id("99999999999999999999"), None);
    assert_eq!(InternalPaginationIds::parse(None, None, Some("x")), None);
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
