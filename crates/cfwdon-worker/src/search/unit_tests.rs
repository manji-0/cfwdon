use crate::search::{
    ParsedStatusSearchQuery, SearchCategoryFlags, SearchUrlQueryMode, SearchV2Query,
    account_matches_search_terms, account_relationship_rank, account_search_is_complete_handle,
    account_search_non_exact_limit, account_search_rank, account_search_resolve_enabled,
    account_search_sort_key, account_search_term, account_search_terms,
    effective_search_v2_following, effective_search_v2_offset, normalize_search_match_text,
    normalize_search_query_input, normalized_account_search_query, parse_status_search_query,
    search_category_flags, search_text_match_rank, search_v2_limit, search_v2_requires_auth,
    search_v2_type_allows_url_resource, search_v2_unauthenticated_error, search_v2_url_query_mode,
    status_is_searchable_by_scope, status_matches_search_metadata, status_matches_search_scope,
    status_matches_search_syntax, status_matches_search_timestamp, status_search_query_terms,
    status_search_rank, text_mentions_search_library_viewer,
};
use crate::test_fixtures::actor_fixture_account;
use cfwdon_core::AppConfig;

#[test]
fn search_category_flags_defaults_to_all_categories() {
    assert_eq!(
        search_category_flags(None),
        SearchCategoryFlags {
            accounts: true,
            statuses: true,
            hashtags: true,
        }
    );
}

#[test]
fn search_category_flags_respects_explicit_type() {
    assert_eq!(
        search_category_flags(Some("accounts")),
        SearchCategoryFlags {
            accounts: true,
            statuses: false,
            hashtags: false,
        }
    );
    assert_eq!(
        search_category_flags(Some("statuses")),
        SearchCategoryFlags {
            accounts: false,
            statuses: true,
            hashtags: false,
        }
    );
    assert_eq!(
        search_category_flags(Some("hashtags")),
        SearchCategoryFlags {
            accounts: false,
            statuses: false,
            hashtags: true,
        }
    );
    assert_eq!(
        search_category_flags(Some(" Accounts ")),
        SearchCategoryFlags {
            accounts: true,
            statuses: false,
            hashtags: false,
        }
    );
}

#[test]
fn search_v2_requires_auth_for_resolve_following_and_offset() {
    assert!(search_v2_requires_auth(&SearchV2Query {
        resolve: Some(true),
        ..SearchV2Query::default()
    }));
    assert!(!search_v2_requires_auth(&SearchV2Query {
        offset: Some(1),
        ..SearchV2Query::default()
    }));
    assert!(search_v2_requires_auth(&SearchV2Query {
        search_type: Some("accounts".to_owned()),
        offset: Some(1),
        ..SearchV2Query::default()
    }));
    assert!(!search_v2_requires_auth(&SearchV2Query {
        following: Some(true),
        ..SearchV2Query::default()
    }));
    assert!(!search_v2_requires_auth(&SearchV2Query::default()));
}

#[test]
fn search_v2_unauthenticated_error_matches_upstream_messages() {
    assert_eq!(
        search_v2_unauthenticated_error(&SearchV2Query {
            offset: Some(1),
            ..SearchV2Query::default()
        }),
        None
    );
    assert_eq!(
        search_v2_unauthenticated_error(&SearchV2Query {
            search_type: Some("accounts".to_owned()),
            offset: Some(1),
            ..SearchV2Query::default()
        }),
        Some("Search queries pagination is not supported without authentication")
    );
    assert_eq!(
        search_v2_unauthenticated_error(&SearchV2Query {
            resolve: Some(true),
            ..SearchV2Query::default()
        }),
        Some(
            "Search queries that resolve remote resources are not supported without authentication"
        )
    );
    assert_eq!(
        search_v2_unauthenticated_error(&SearchV2Query {
            following: Some(true),
            ..SearchV2Query::default()
        }),
        None
    );
}

#[test]
fn effective_search_v2_offset_ignores_untyped_offset() {
    assert_eq!(
        effective_search_v2_offset(&SearchV2Query {
            offset: Some(10),
            ..SearchV2Query::default()
        }),
        0
    );
    assert_eq!(
        effective_search_v2_offset(&SearchV2Query {
            search_type: Some("hashtags".to_owned()),
            offset: Some(10),
            ..SearchV2Query::default()
        }),
        10
    );
    assert_eq!(
        effective_search_v2_offset(&SearchV2Query {
            search_type: Some(" Hashtags ".to_owned()),
            offset: Some(10),
            ..SearchV2Query::default()
        }),
        10
    );
}

#[test]
fn effective_search_v2_following_requires_authenticated_viewer() {
    assert!(!effective_search_v2_following(
        &SearchV2Query {
            following: Some(true),
            ..SearchV2Query::default()
        },
        false
    ));
    assert!(effective_search_v2_following(
        &SearchV2Query {
            following: Some(true),
            ..SearchV2Query::default()
        },
        true
    ));
    assert!(!effective_search_v2_following(
        &SearchV2Query::default(),
        true
    ));
}

#[test]
fn search_v2_limit_matches_mastodon_bounds() {
    assert_eq!(search_v2_limit(None), 20);
    assert_eq!(search_v2_limit(Some(0)), 1);
    assert_eq!(search_v2_limit(Some(5)), 5);
    assert_eq!(search_v2_limit(Some(80)), 40);
}

#[test]
fn search_v2_url_query_mode_matches_mastodon_url_resolution_rules() {
    assert_eq!(
        search_v2_url_query_mode("https://remote.example/@alice", true, 0),
        SearchUrlQueryMode::ResolveOnly
    );
    assert_eq!(
        search_v2_url_query_mode("https://remote.example/@alice", true, 1),
        SearchUrlQueryMode::EmptyResults
    );
    assert_eq!(
        search_v2_url_query_mode("https://remote.example/@alice", false, 0),
        SearchUrlQueryMode::None
    );
    assert_eq!(
        search_v2_url_query_mode("@alice@remote.example", true, 0),
        SearchUrlQueryMode::None
    );
}

#[test]
fn search_v2_type_allows_url_resource_matches_requested_category() {
    assert!(search_v2_type_allows_url_resource(None, "accounts"));
    assert!(search_v2_type_allows_url_resource(
        Some("accounts"),
        "accounts"
    ));
    assert!(!search_v2_type_allows_url_resource(
        Some("statuses"),
        "accounts"
    ));
    assert!(!search_v2_type_allows_url_resource(
        Some("hashtags"),
        "statuses"
    ));
    assert!(!search_v2_type_allows_url_resource(
        Some("other"),
        "accounts"
    ));
}

#[test]
fn status_search_query_terms_include_all_candidate_terms() {
    let parsed = parse_status_search_query(r#"rust "release notes" from:me -is:reply"#);
    assert_eq!(
        status_search_query_terms(&parsed),
        vec![
            "rust".to_owned(),
            "release notes".to_owned(),
            "rust release notes".to_owned(),
        ]
    );
}

#[test]
fn search_text_match_rank_prefers_exact_then_prefix_then_contains() {
    assert_eq!(search_text_match_rank("alice", "alice"), 0);
    assert_eq!(search_text_match_rank("ali", "alice"), 1);
    assert_eq!(search_text_match_rank("lic", "alice"), 2);
    assert_eq!(search_text_match_rank("bob", "alice"), 3);
}

#[test]
fn normalize_search_match_text_folds_case_quotes_and_latin_accents() {
    assert_eq!(
        normalize_search_match_text("「Café」 Résumé München Straße"),
        "\"cafe\" resume munchen strasse"
    );
}

#[test]
fn search_text_match_rank_matches_folded_latin_accents() {
    assert_eq!(search_text_match_rank("cafe", "Café"), 0);
    assert_eq!(search_text_match_rank("resume", "résumé update"), 1);
    assert_eq!(search_text_match_rank("strasse", "die Straße"), 2);
}

#[test]
fn account_matches_search_terms_matches_folded_latin_accents() {
    assert!(account_matches_search_terms(
        &["cafe".to_owned(), "resume".to_owned()],
        "alice",
        "alice@example.com",
        "Café Alice",
        "résumé posts"
    ));
}

#[test]
fn normalize_search_query_input_maps_quote_equivalents_to_ascii_quotes() {
    assert_eq!(
        normalize_search_query_input("「release」 “notes”"),
        "\"release\" \"notes\""
    );
}

#[test]
fn normalized_account_search_query_supports_handles() {
    assert_eq!(normalized_account_search_query("@alice"), "alice");
    assert_eq!(
        normalized_account_search_query("acct:alice@remote.example"),
        "alice@remote.example"
    );
    assert_eq!(
        normalized_account_search_query("@alice@remote.example"),
        "alice@remote.example"
    );
    assert_eq!(
        normalized_account_search_query("ACCT:Alice@Remote.Example"),
        "alice@remote.example"
    );
}

#[test]
fn account_search_is_complete_handle_requires_domain_form() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert!(account_search_is_complete_handle(
        "@alice@remote.example",
        &config
    ));
    assert!(account_search_is_complete_handle(
        "acct:alice@remote.example",
        &config
    ));
    assert!(!account_search_is_complete_handle("alice", &config));
    assert!(!account_search_is_complete_handle("@alice", &config));
    assert!(!account_search_is_complete_handle("hi @alice", &config));
    assert!(!account_search_is_complete_handle(
        "alice @remote.example",
        &config
    ));
}

#[test]
fn account_search_resolve_enabled_defaults_complete_handles() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert!(account_search_resolve_enabled(
        None,
        "@alice@remote.example",
        &config
    ));
    assert!(!account_search_resolve_enabled(
        Some(false),
        "@alice@remote.example",
        &config
    ));
    assert!(account_search_resolve_enabled(Some(true), "alice", &config));
    assert!(!account_search_resolve_enabled(None, "alice", &config));
}

#[test]
fn account_search_term_treats_local_domain_handles_as_usernames() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert_eq!(account_search_term("alice", &config), "alice");
    assert_eq!(
        account_search_term("@alice@social.example", &config),
        "alice"
    );
    assert_eq!(
        account_search_term("acct:alice@remote.example", &config),
        "alice@remote.example"
    );
}

#[test]
fn account_search_terms_split_words_and_keep_quoted_phrases() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert_eq!(
        account_search_terms("alice rust", &config),
        vec!["alice".to_owned(), "rust".to_owned()]
    );
    assert_eq!(
        account_search_terms("alice \"rust workers\"", &config),
        vec!["alice".to_owned(), "rust workers".to_owned()]
    );
}

#[test]
fn account_matches_search_terms_requires_all_terms_across_profile_fields() {
    assert!(account_matches_search_terms(
        &["alice".to_owned(), "workers".to_owned()],
        "alice",
        "alice",
        "Alice",
        "workers and rust"
    ));
    assert!(!account_matches_search_terms(
        &["alice".to_owned(), "workers".to_owned()],
        "alice",
        "alice",
        "Alice",
        ""
    ));
}

#[test]
fn account_search_non_exact_limit_matches_mastodon_rules() {
    let viewer = actor_fixture_account();
    assert_eq!(account_search_non_exact_limit("ab", None, 20, false), 0);
    assert_eq!(account_search_non_exact_limit("#rust", None, 20, false), 0);
    assert_eq!(
        account_search_non_exact_limit("#rust", Some(&viewer), 20, false),
        0
    );
    assert_eq!(
        account_search_non_exact_limit("@alice@remote.example", None, 20, true),
        19
    );
    assert_eq!(
        account_search_non_exact_limit("ab", Some(&viewer), 20, false),
        20
    );
}

#[test]
fn account_search_rank_prefers_exact_acct_for_handle_queries() {
    assert!(
        account_search_rank(
            "alice@remote.example",
            "alice",
            "alice@remote.example",
            "Alice",
            ""
        ) < account_search_rank(
            "alice@remote.example",
            "alice",
            "alice@another.example",
            "Alice",
            ""
        )
    );
}

#[test]
fn account_relationship_rank_prefers_self_then_following() {
    assert!(account_relationship_rank(true, false) < account_relationship_rank(false, true));
    assert!(account_relationship_rank(false, true) < account_relationship_rank(false, false));
}

#[test]
fn account_search_sort_key_uses_relationship_rank_as_tiebreaker() {
    assert!(
        account_search_sort_key("alice", "alice", "alice", "Alice", "", 0, 0, 0)
            < account_search_sort_key("alice", "alice", "alice", "Alice", "", 1, 0, 0)
    );
    assert!(
        account_search_sort_key("alice", "alice", "alice", "Alice", "", 1, 0, 0)
            < account_search_sort_key("alice", "alice", "alice", "Alice", "", 2, 0, 0)
    );
}

#[test]
fn account_search_sort_key_prefers_more_popular_accounts_on_tie() {
    assert!(
        account_search_sort_key("alice", "alice", "alice", "Alice", "", 2, 100, 5)
            < account_search_sort_key("alice", "alice", "alice", "Alice", "", 2, 10, 5)
    );
    assert!(
        account_search_sort_key("alice", "alice", "alice", "Alice", "", 2, 10, 20)
            < account_search_sort_key("alice", "alice", "alice", "Alice", "", 2, 10, 5)
    );
}

#[test]
fn account_search_rank_considers_profile_note_after_names() {
    assert!(
        account_search_rank("workers", "alice", "alice", "Alice", "workers and rust")
            < account_search_rank("workers", "alice", "alice", "Alice", "")
    );
    assert!(
        account_search_rank("alice", "alice", "alice", "Alice", "alice in bio")
            < account_search_rank("alice", "zzz", "zzz", "zzz", "alice in bio")
    );
}

#[test]
fn account_search_rank_prefers_multi_term_coverage_before_partial_matches() {
    assert!(
        account_search_rank("alice rust", "alice", "alice", "Alice Rust", "")
            < account_search_rank("alice rust", "alice", "alice", "Alice", "")
    );
    assert!(
        account_search_rank(
            "\"rust workers\"",
            "alice",
            "alice",
            "Alice",
            "rust workers"
        ) < account_search_rank("\"rust workers\"", "alice", "alice", "Alice", "rust")
    );
}

#[test]
fn status_search_rank_prefers_content_matches_before_spoilers() {
    let rust_query = parse_status_search_query("rust");
    assert!(
        status_search_rank(&rust_query, "rust release notes", "cw")
            < status_search_rank(&rust_query, "cw", "rust release notes")
    );
    assert!(
        status_search_rank(&rust_query, "rust release notes", "cw")
            < status_search_rank(&rust_query, "fedi post", "cw")
    );
    let rust_release_query = parse_status_search_query("rust release");
    assert!(
        status_search_rank(&rust_release_query, "rust release notes", "cw")
            < status_search_rank(&rust_release_query, "rust notes", "release candidate")
    );
    assert!(
        status_search_rank(&rust_release_query, "rust notes", "release candidate")
            < status_search_rank(&rust_release_query, "rust notes only", "cw")
    );
    let mixed_phrase_query = parse_status_search_query("foo \"bar baz\"");
    assert!(
        status_search_rank(&mixed_phrase_query, "foo update with bar baz", "")
            < status_search_rank(&mixed_phrase_query, "foo update with bar and baz", "")
    );
}

#[test]
fn parse_status_search_query_extracts_basic_status_syntax_filters() {
    assert_eq!(
        parse_status_search_query("rust release from:me before:\"2025-03-01\" after:2025-02-01"),
        ParsedStatusSearchQuery {
            text_query: "rust release".to_owned(),
            included_text_terms: vec!["rust".to_owned(), "release".to_owned()],
            excluded_text_terms: Vec::new(),
            from: Some("me".to_owned()),
            not_from: None,
            before: Some("2025-03-01T00:00:00Z".to_owned()),
            after: Some("2025-02-01T00:00:00Z".to_owned()),
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: None,
            not_language: None,
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_expands_during_into_day_bounds() {
    assert_eq!(
        parse_status_search_query("\"rust release\" during:2025-03-01"),
        ParsedStatusSearchQuery {
            text_query: "rust release".to_owned(),
            included_text_terms: vec!["rust release".to_owned()],
            excluded_text_terms: Vec::new(),
            from: None,
            not_from: None,
            before: Some("2025-03-02T00:00:00Z".to_owned()),
            after: Some("2025-03-01T00:00:00Z".to_owned()),
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: None,
            not_language: None,
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_accepts_epoch_timestamps() {
    assert_eq!(
        parse_status_search_query("before:1740873600 after:1740787200 during:1740787200"),
        ParsedStatusSearchQuery {
            text_query: String::new(),
            included_text_terms: Vec::new(),
            excluded_text_terms: Vec::new(),
            from: None,
            not_from: None,
            before: Some("2025-03-01T00:00:00Z".to_owned()),
            after: Some("2025-03-01T00:00:00Z".to_owned()),
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: None,
            not_language: None,
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_extracts_negated_date_filters() {
    assert_eq!(
        parse_status_search_query("-before:\"2025-03-01\" -after:2025-02-01 -during:2025-02-10"),
        ParsedStatusSearchQuery {
            text_query: String::new(),
            included_text_terms: Vec::new(),
            excluded_text_terms: Vec::new(),
            from: None,
            not_from: None,
            before: None,
            after: None,
            excluded_before: Some("2025-03-01T00:00:00Z".to_owned()),
            excluded_after: Some("2025-02-01T00:00:00Z".to_owned()),
            excluded_during: vec![(
                "2025-02-10T00:00:00Z".to_owned(),
                "2025-02-11T00:00:00Z".to_owned(),
            )],
            language: None,
            not_language: None,
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_normalizes_quote_equivalent_characters() {
    assert_eq!(
        parse_status_search_query("rust 「release notes」 -“outage”"),
        ParsedStatusSearchQuery {
            text_query: "rust release notes".to_owned(),
            included_text_terms: vec!["rust".to_owned(), "release notes".to_owned()],
            excluded_text_terms: vec!["outage".to_owned()],
            from: None,
            not_from: None,
            before: None,
            after: None,
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: None,
            not_language: None,
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_preserves_escaped_quote_and_space_terms() {
    assert_eq!(
        parse_status_search_query(r#"rust "release \"notes\"" escaped\ space"#),
        ParsedStatusSearchQuery {
            text_query: "rust release \"notes\" escaped space".to_owned(),
            included_text_terms: vec![
                "rust".to_owned(),
                "release \"notes\"".to_owned(),
                "escaped space".to_owned()
            ],
            excluded_text_terms: Vec::new(),
            from: None,
            not_from: None,
            before: None,
            after: None,
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: None,
            not_language: None,
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_keeps_non_special_backslashes() {
    assert_eq!(
        parse_status_search_query(r#"path\name"#).included_text_terms,
        vec![r#"path\name"#.to_owned()]
    );
}

#[test]
fn parse_status_search_query_extracts_language_is_and_has_filters() {
    assert_eq!(
        parse_status_search_query(
            "rust -\"remote outage\" language:ja -language:en from:me -from:bob is:reply -is:sensitive is:boost -is:quote has:media -has:poll has:embed in:public -in:library"
        ),
        ParsedStatusSearchQuery {
            text_query: "rust".to_owned(),
            included_text_terms: vec!["rust".to_owned()],
            excluded_text_terms: vec!["remote outage".to_owned()],
            from: Some("me".to_owned()),
            not_from: Some("bob".to_owned()),
            before: None,
            after: None,
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: Some("ja".to_owned()),
            not_language: Some("en".to_owned()),
            is_reply: Some(true),
            is_sensitive: Some(false),
            is_boost: Some(true),
            is_quote: Some(false),
            has_media: Some(true),
            has_poll: Some(false),
            has_embed: Some(true),
            in_public: Some(true),
            in_library: Some(false),
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_accepts_advanced_search_aliases() {
    let parsed = parse_status_search_query("is:reblog -is:quote has:link -has:preview");

    assert_eq!(parsed.is_boost, Some(true));
    assert_eq!(parsed.is_quote, Some(false));
    assert!(parsed.unsatisfiable);
}

#[test]
fn parse_status_search_query_accepts_explicit_positive_operator() {
    assert_eq!(
        parse_status_search_query(
            "+rust +\"release notes\" +from:me +language:ja +has:media +in:public"
        ),
        ParsedStatusSearchQuery {
            text_query: "rust release notes".to_owned(),
            included_text_terms: vec!["rust".to_owned(), "release notes".to_owned()],
            excluded_text_terms: Vec::new(),
            from: Some("me".to_owned()),
            not_from: None,
            before: None,
            after: None,
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: Some("ja".to_owned()),
            not_language: None,
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: Some(true),
            has_poll: None,
            has_embed: None,
            in_public: Some(true),
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_treats_prefixes_case_insensitively() {
    assert_eq!(
        parse_status_search_query(
            "Rust FROM:Me Language:EN-us IS:Reply HAS:Media IN:Library Site:Example.com"
        ),
        ParsedStatusSearchQuery {
            text_query: "Rust site Example.com".to_owned(),
            included_text_terms: vec!["Rust".to_owned(), "site Example.com".to_owned()],
            excluded_text_terms: Vec::new(),
            from: Some("Me".to_owned()),
            not_from: None,
            before: None,
            after: None,
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: Some("en".to_owned()),
            not_language: None,
            is_reply: Some(true),
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: Some(true),
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: Some(true),
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_falls_back_unknown_prefixes_to_text_terms() {
    assert_eq!(
        parse_status_search_query("cryptid site:example.com -mood:spooky"),
        ParsedStatusSearchQuery {
            text_query: "cryptid site example.com".to_owned(),
            included_text_terms: vec!["cryptid".to_owned(), "site example.com".to_owned()],
            excluded_text_terms: vec!["mood spooky".to_owned()],
            from: None,
            not_from: None,
            before: None,
            after: None,
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: None,
            not_language: None,
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn parse_status_search_query_marks_conflicting_filters_unsatisfiable() {
    assert_eq!(
        parse_status_search_query("from:alice from:bob is:reply -is:reply"),
        ParsedStatusSearchQuery {
            text_query: String::new(),
            included_text_terms: Vec::new(),
            excluded_text_terms: Vec::new(),
            from: Some("alice".to_owned()),
            not_from: None,
            before: None,
            after: None,
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: None,
            not_language: None,
            is_reply: Some(true),
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: true,
        }
    );
}

#[test]
fn parse_status_search_query_normalizes_language_subtags() {
    assert_eq!(
        parse_status_search_query("language:EN-us -language:pt_BR"),
        ParsedStatusSearchQuery {
            text_query: String::new(),
            included_text_terms: Vec::new(),
            excluded_text_terms: Vec::new(),
            from: None,
            not_from: None,
            before: None,
            after: None,
            excluded_before: None,
            excluded_after: None,
            excluded_during: Vec::new(),
            language: Some("en".to_owned()),
            not_language: Some("pt".to_owned()),
            is_reply: None,
            is_sensitive: None,
            is_boost: None,
            is_quote: None,
            has_media: None,
            has_poll: None,
            has_embed: None,
            in_public: None,
            in_library: None,
            unsatisfiable: false,
        }
    );
}

#[test]
fn status_matches_search_syntax_applies_language_and_is_filters() {
    let parsed =
        parse_status_search_query("language:ja -language:en is:reply -is:sensitive -blocked");
    assert!(status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        true,
        false,
        false,
        false,
        Some("ja")
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        false,
        false,
        false,
        false,
        Some("ja")
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        true,
        true,
        false,
        false,
        Some("ja")
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        true,
        false,
        false,
        false,
        Some("en")
    ));
    assert!(status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        true,
        false,
        false,
        false,
        Some("ja-JP")
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        true,
        false,
        false,
        false,
        Some("en-US")
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "blocked release notes",
        "",
        true,
        false,
        false,
        false,
        Some("ja")
    ));
}

#[test]
fn status_matches_search_syntax_requires_all_positive_text_terms() {
    let parsed = parse_status_search_query("rust release");
    assert!(status_matches_search_syntax(
        &parsed,
        "rust release notes",
        "",
        false,
        false,
        false,
        false,
        None
    ));
    assert!(status_matches_search_syntax(
        &parsed,
        "rust notes",
        "release candidate",
        false,
        false,
        false,
        false,
        None
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "rust notes only",
        "",
        false,
        false,
        false,
        false,
        None
    ));
}

#[test]
fn status_matches_search_syntax_applies_boost_and_quote_filters() {
    let parsed = parse_status_search_query("is:boost -is:quote");

    assert!(status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        false,
        false,
        true,
        false,
        None
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        false,
        false,
        false,
        false,
        None
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "release notes",
        "",
        false,
        false,
        true,
        true,
        None
    ));
}

#[test]
fn status_matches_search_timestamp_applies_negated_date_filters() {
    let negated_before = parse_status_search_query("-before:\"2025-03-01\"");
    assert!(status_matches_search_timestamp(
        &negated_before,
        "2025-03-01T00:00:00Z"
    ));
    assert!(!status_matches_search_timestamp(
        &negated_before,
        "2025-02-28T23:59:59Z"
    ));

    let negated_after = parse_status_search_query("-after:2025-02-01");
    assert!(status_matches_search_timestamp(
        &negated_after,
        "2025-02-01T00:00:00Z"
    ));
    assert!(!status_matches_search_timestamp(
        &negated_after,
        "2025-02-01T00:00:01Z"
    ));

    let negated_during = parse_status_search_query("-during:2025-02-10");
    assert!(status_matches_search_timestamp(
        &negated_during,
        "2025-02-09T23:59:59Z"
    ));
    assert!(!status_matches_search_timestamp(
        &negated_during,
        "2025-02-10T12:00:00Z"
    ));
}

#[test]
fn status_matches_search_syntax_treats_hashtag_terms_as_tags() {
    let parsed = parse_status_search_query("#rust -#blocked");
    assert!(status_matches_search_syntax(
        &parsed,
        "release notes for #Rust",
        "",
        false,
        false,
        false,
        false,
        None
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "rust release notes",
        "",
        false,
        false,
        false,
        false,
        None
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "release notes for #rust #blocked",
        "",
        false,
        false,
        false,
        false,
        None
    ));
}

#[test]
fn status_matches_search_syntax_matches_folded_latin_accents() {
    let parsed = parse_status_search_query("cafe -resume");
    assert!(status_matches_search_syntax(
        &parsed,
        "Café notes",
        "",
        false,
        false,
        false,
        false,
        None
    ));
    assert!(!status_matches_search_syntax(
        &parsed,
        "Café résumé notes",
        "",
        false,
        false,
        false,
        false,
        None
    ));
}

#[test]
fn status_matches_search_metadata_applies_has_filters() {
    let parsed = parse_status_search_query("has:media -has:poll has:embed");
    assert!(status_matches_search_metadata(&parsed, true, false, true));
    assert!(!status_matches_search_metadata(&parsed, false, false, true));
    assert!(!status_matches_search_metadata(&parsed, true, true, true));
    assert!(!status_matches_search_metadata(&parsed, true, false, false));
}

#[test]
fn status_matches_search_scope_applies_in_public_filter() {
    let public_only = parse_status_search_query("in:public");
    assert!(status_matches_search_scope(&public_only, true, false));
    assert!(!status_matches_search_scope(&public_only, false, false));

    let non_public_only = parse_status_search_query("-in:public");
    assert!(status_matches_search_scope(&non_public_only, false, false));
    assert!(!status_matches_search_scope(&non_public_only, true, false));
}

#[test]
fn status_matches_search_scope_applies_in_library_filter() {
    let library_only = parse_status_search_query("in:library");
    assert!(status_matches_search_scope(&library_only, false, true));
    assert!(!status_matches_search_scope(&library_only, true, false));

    let outside_library_only = parse_status_search_query("-in:library");
    assert!(status_matches_search_scope(
        &outside_library_only,
        true,
        false
    ));
    assert!(!status_matches_search_scope(
        &outside_library_only,
        true,
        true
    ));
}

#[test]
fn status_is_searchable_by_scope_defaults_to_public_plus_library() {
    let default_query = parse_status_search_query("rust");
    assert!(status_is_searchable_by_scope(&default_query, true, false));
    assert!(status_is_searchable_by_scope(&default_query, false, true));
    assert!(!status_is_searchable_by_scope(&default_query, false, false));

    let public_only = parse_status_search_query("in:public");
    assert!(!status_is_searchable_by_scope(&public_only, false, true));

    let library_only = parse_status_search_query("in:library");
    assert!(!status_is_searchable_by_scope(&library_only, true, false));
}

#[test]
fn text_mentions_search_library_viewer_detects_local_and_remote_handles() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    assert!(text_mentions_search_library_viewer(
        &config,
        "@alice thanks for the report",
        "alice"
    ));
    assert!(text_mentions_search_library_viewer(
        &config,
        "@alice@social.example thanks for the report",
        "alice"
    ));
    assert!(!text_mentions_search_library_viewer(
        &config,
        "@bob thanks for the report",
        "alice"
    ));
}
