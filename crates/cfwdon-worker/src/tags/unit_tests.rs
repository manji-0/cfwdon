use crate::tags::{
    TagSearchMetrics, paginate_tag_search_matches, resolve_search_tag_name,
    tag_matches_search_query, tag_search_rank, tag_search_sort_key,
};

#[test]
fn tag_search_rank_prefers_exact_matches() {
    assert!(tag_search_rank("rust", "rust") < tag_search_rank("rust", "rustlang"));
    assert!(tag_search_rank("rust", "rustlang") < tag_search_rank("rust", "fedirust"));
}

#[test]
fn tag_matches_search_query_uses_prefix_semantics() {
    assert!(tag_matches_search_query("rust", "rustlang"));
    assert!(tag_matches_search_query("#rust", "rustlang"));
    assert!(!tag_matches_search_query("rust", "fedirust"));
}

#[test]
fn tag_search_matches_folded_latin_accents() {
    assert_eq!(tag_search_rank("cafe", "Café").0, 0);
    assert!(tag_matches_search_query("munchen", "München"));
}

#[test]
fn tag_search_sort_key_prefers_usage_then_recency_on_match_ties() {
    assert!(
        tag_search_sort_key("rust", "rustacean", 100, Some("2026-04-21"))
            < tag_search_sort_key("rust", "rustlang", 10, Some("2026-04-22"))
    );
    assert!(
        tag_search_sort_key("rust", "rustacean", 10, Some("2026-04-21"))
            < tag_search_sort_key("rust", "rustlang", 10, Some("2026-04-20"))
    );
}

#[test]
fn paginate_tag_search_matches_applies_offset_after_usage_aware_ranking() {
    let tags = vec![
        (
            "fedirust".to_owned(),
            TagSearchMetrics {
                statuses_count: 5,
                accounts_count: 2,
                last_status_at: Some("2026-04-18".to_owned()),
            },
        ),
        (
            "rust".to_owned(),
            TagSearchMetrics {
                statuses_count: 1,
                accounts_count: 1,
                last_status_at: Some("2026-04-17".to_owned()),
            },
        ),
        (
            "rustlang".to_owned(),
            TagSearchMetrics {
                statuses_count: 20,
                accounts_count: 5,
                last_status_at: Some("2026-04-20".to_owned()),
            },
        ),
        (
            "rustacean".to_owned(),
            TagSearchMetrics {
                statuses_count: 20,
                accounts_count: 4,
                last_status_at: Some("2026-04-21".to_owned()),
            },
        ),
    ];

    assert_eq!(
        paginate_tag_search_matches("rust", tags.clone(), 2, 0),
        vec![
            (
                "rust".to_owned(),
                TagSearchMetrics {
                    statuses_count: 1,
                    accounts_count: 1,
                    last_status_at: Some("2026-04-17".to_owned()),
                },
            ),
            (
                "rustacean".to_owned(),
                TagSearchMetrics {
                    statuses_count: 20,
                    accounts_count: 4,
                    last_status_at: Some("2026-04-21".to_owned()),
                },
            ),
        ]
    );
    assert_eq!(
        paginate_tag_search_matches("rust", tags, 2, 1),
        vec![
            (
                "rustacean".to_owned(),
                TagSearchMetrics {
                    statuses_count: 20,
                    accounts_count: 4,
                    last_status_at: Some("2026-04-21".to_owned()),
                },
            ),
            (
                "rustlang".to_owned(),
                TagSearchMetrics {
                    statuses_count: 20,
                    accounts_count: 5,
                    last_status_at: Some("2026-04-20".to_owned()),
                },
            ),
        ]
    );
}

#[test]
fn resolve_search_tag_name_supports_hash_and_tag_urls() {
    assert_eq!(resolve_search_tag_name("#Rust"), Some("rust".to_owned()));
    assert_eq!(
        resolve_search_tag_name("https://social.example/tags/Rust"),
        Some("rust".to_owned())
    );
    assert_eq!(
        resolve_search_tag_name("https://social.example/explore/tags/Workers"),
        Some("workers".to_owned())
    );
    assert_eq!(
        resolve_search_tag_name("/tags/fediverse_test"),
        Some("fediverse_test".to_owned())
    );
    assert_eq!(
        resolve_search_tag_name("https://social.example/Tags/Rust"),
        Some("rust".to_owned())
    );
    assert_eq!(
        resolve_search_tag_name("https://social.example/Explore/Tags/Workers"),
        Some("workers".to_owned())
    );
    assert_eq!(
        resolve_search_tag_name("https://social.example/tags/Rust%20Lang"),
        Some("rust lang".to_owned())
    );
}

#[test]
fn resolve_search_tag_name_rejects_non_tag_queries() {
    assert_eq!(resolve_search_tag_name("rust"), None);
    assert_eq!(
        resolve_search_tag_name("https://social.example/@alice"),
        None
    );
    assert_eq!(resolve_search_tag_name(""), None);
}
