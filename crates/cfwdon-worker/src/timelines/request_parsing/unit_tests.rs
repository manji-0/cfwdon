use crate::timelines::request_parsing::build_timeline_link_header_for_url;
use url::Url;

#[test]
fn build_timeline_link_header_preserves_non_cursor_filters() {
    let url = Url::parse(
        "https://example.com/api/v1/timelines/tag/rust?limit=1&local=true&any[]=timeline&max_id=old",
    )
    .unwrap();
    let header =
        build_timeline_link_header_for_url(&url, 20, Some("newest"), Some("oldest")).unwrap();
    assert!(header.contains("local=true"));
    assert!(header.contains("any%5B%5D=timeline"));
    assert!(header.contains("max_id=oldest"));
    assert!(header.contains("min_id=newest"));
    assert!(!header.contains("max_id=old&"));
}
