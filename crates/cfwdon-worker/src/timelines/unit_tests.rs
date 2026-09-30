use crate::timelines::{
    HomeTimelineQuery, LinkTimelineQuery, PublicTimelineQuery, TagTimelineQuery,
    TimelinePaginationQuery, derive_link_timeline_match_urls, include_local_source,
    include_remote_source, matches_tag_timeline_filters, timeline_fetch_limit, timeline_limit,
};

#[test]
fn matches_tag_timeline_filters_applies_any_all_none() {
    let tags = vec![
        "rust".to_owned(),
        "workers".to_owned(),
        "activitypub".to_owned(),
    ];
    assert!(matches_tag_timeline_filters(
        &tags,
        "rust",
        &TagTimelineQuery::default()
    ));
    assert!(matches_tag_timeline_filters(
        &tags,
        "rust",
        &TagTimelineQuery {
            any: Some(vec!["workers".to_owned(), "d1".to_owned()]),
            all: Some(vec!["activitypub".to_owned()]),
            ..TagTimelineQuery::default()
        }
    ));
    assert!(!matches_tag_timeline_filters(
        &tags,
        "rust",
        &TagTimelineQuery {
            none: Some(vec!["workers".to_owned()]),
            ..TagTimelineQuery::default()
        }
    ));
}

#[test]
fn tag_timeline_source_flags_default_to_both_sources() {
    assert!(include_local_source(None, None));
    assert!(include_remote_source(None, None));
    assert!(include_local_source(Some(true), Some(false)));
    assert!(!include_remote_source(Some(true), Some(false)));
    assert!(!include_local_source(Some(false), Some(true)));
    assert!(include_remote_source(Some(false), Some(true)));
}

#[test]
fn timeline_fetch_limit_caps_oversampling_window() {
    assert_eq!(timeline_fetch_limit(1), 4);
    assert_eq!(timeline_fetch_limit(20), 80);
    assert_eq!(timeline_fetch_limit(40), 160);
}

#[test]
fn timeline_limit_clamps_requested_page_size() {
    assert_eq!(
        timeline_limit(&TimelinePaginationQuery {
            limit: None,
            ..TimelinePaginationQuery::default()
        }),
        20
    );
    assert_eq!(
        timeline_limit(&TimelinePaginationQuery {
            limit: Some(0),
            ..TimelinePaginationQuery::default()
        }),
        1
    );
    assert_eq!(
        timeline_limit(&TimelinePaginationQuery {
            limit: Some(80),
            ..TimelinePaginationQuery::default()
        }),
        40
    );
}

#[test]
fn timeline_query_strings_populate_pagination_fields() {
    let public: PublicTimelineQuery =
        serde_urlencoded::from_str("limit=3&max_id=older&since_id=newer&local=true").unwrap();
    assert_eq!(
        public.pagination(),
        TimelinePaginationQuery {
            limit: Some(3),
            max_id: Some("older".to_owned()),
            since_id: Some("newer".to_owned()),
            min_id: None,
        }
    );
    assert_eq!(public.local, Some(true));

    let home: HomeTimelineQuery = serde_urlencoded::from_str("limit=20&max_id=older-home").unwrap();
    assert_eq!(
        home.pagination(),
        TimelinePaginationQuery {
            limit: Some(20),
            max_id: Some("older-home".to_owned()),
            since_id: None,
            min_id: None,
        }
    );

    let tag: TagTimelineQuery = serde_urlencoded::from_str("limit=5&min_id=fresh").unwrap();
    assert_eq!(
        tag.pagination(),
        TimelinePaginationQuery {
            limit: Some(5),
            max_id: None,
            since_id: None,
            min_id: Some("fresh".to_owned()),
        }
    );

    let link: LinkTimelineQuery =
        serde_urlencoded::from_str("url=https%3A%2F%2Fexample.com&since_id=fresh-link").unwrap();
    assert_eq!(
        link.pagination(),
        TimelinePaginationQuery {
            limit: None,
            max_id: None,
            since_id: Some("fresh-link".to_owned()),
            min_id: None,
        }
    );
    assert_eq!(link.url.as_deref(), Some("https://example.com"));
}

#[test]
fn derive_link_timeline_match_urls_normalizes_fragment_and_trailing_slash() {
    assert_eq!(
        derive_link_timeline_match_urls(" https://Example.com/articles/rust#intro "),
        vec![
            "https://Example.com/articles/rust#intro".to_owned(),
            "https://example.com/articles/rust".to_owned(),
            "https://example.com/articles/rust/".to_owned(),
        ]
    );
}

#[test]
fn derive_link_timeline_match_urls_removes_tracking_query_params() {
    assert_eq!(
        derive_link_timeline_match_urls(
            "https://example.com/articles/rust?utm_source=mastodon&fbclid=abc123"
        ),
        vec![
            "https://example.com/articles/rust?utm_source=mastodon&fbclid=abc123".to_owned(),
            "https://example.com/articles/rust".to_owned(),
            "https://example.com/articles/rust/".to_owned(),
            "https://example.com/articles/rust/?utm_source=mastodon&fbclid=abc123".to_owned(),
        ]
    );
}

#[test]
fn derive_link_timeline_match_urls_keeps_invalid_url_as_is() {
    assert_eq!(
        derive_link_timeline_match_urls("not a url"),
        vec!["not a url".to_owned()]
    );
}
