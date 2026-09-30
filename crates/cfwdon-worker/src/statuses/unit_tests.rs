use crate::statuses::{
    AccountStatusesQuery, TranslationProviderLanguageRow, apply_html_preview_metadata,
    build_deepl_translation_languages_document, build_status_card_value,
    build_translation_document, build_translation_document_for_language,
    build_translation_languages_document, effective_local_quote_approval_policy,
    effective_status_quote_state, extract_html_preview_metadata, first_url_from_text,
    local_quote_policy_allows, local_quote_revoke_allowed, local_status_allows_viewer,
    normalize_quote_approval_policy, normalize_scheduled_at, normalize_status_history_entry,
    normalized_action_uri, pending_quote_document, quote_document_with_state,
    quote_placeholder_document, status_has_active_quote, translation_provider_language_code,
    translation_provider_supported_target_language, translation_target_language,
    validate_scheduled_at_minimum_offset,
};
use cfwdon_domain::LocalStatus;

#[test]
fn normalize_scheduled_at_accepts_rfc3339() {
    assert_eq!(
        normalize_scheduled_at(Some("2099-02-03T04:05:06Z")).unwrap(),
        Some("2099-02-03T04:05:06Z".to_owned())
    );
}

#[test]
fn normalize_scheduled_at_rejects_invalid_timestamp() {
    assert!(normalize_scheduled_at(Some("2099/02/03 04:05:06")).is_err());
}

#[test]
fn validate_scheduled_at_minimum_offset_rejects_too_soon_timestamp() {
    let soon = (time::OffsetDateTime::now_utc() + time::Duration::minutes(4))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let error = validate_scheduled_at_minimum_offset(&soon).unwrap_err();

    assert!(error.contains("Scheduled at"));
}

#[test]
fn validate_scheduled_at_minimum_offset_accepts_future_timestamp() {
    let later = (time::OffsetDateTime::now_utc() + time::Duration::minutes(6))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();

    validate_scheduled_at_minimum_offset(&later).unwrap();
}

#[test]
fn translation_document_matches_upstream_shape() {
    let document = build_translation_document(&serde_json::json!({
        "content": "<p>Hello world</p>",
        "spoiler_text": "cw",
        "language": "ja",
        "media_attachments": [
            {
                "id": "media-1",
                "description": "alt text",
                "url": "https://media.example/media-1"
            }
        ],
        "poll": {
            "id": "poll-1",
            "options": [
                { "title": "One", "votes_count": 1 },
                { "title": "Two", "votes_count": 2 }
            ]
        }
    }));

    assert_eq!(
        document.pointer("/language"),
        Some(&serde_json::json!("ja"))
    );
    assert_eq!(
        document.pointer("/detected_source_language"),
        Some(&serde_json::json!("ja"))
    );
    assert_eq!(
        document.pointer("/media_attachments/0/id"),
        Some(&serde_json::json!("media-1"))
    );
    assert_eq!(
        document.pointer("/media_attachments/0/description"),
        Some(&serde_json::json!("alt text"))
    );
    assert_eq!(
        document.pointer("/poll/id"),
        Some(&serde_json::json!("poll-1"))
    );
    assert_eq!(
        document.pointer("/poll/options/1/title"),
        Some(&serde_json::json!("Two"))
    );
}

#[test]
fn translation_document_for_language_overrides_target_language() {
    let document = build_translation_document_for_language(
        &serde_json::json!({
            "content": "<p>Hello</p>",
            "spoiler_text": "",
            "language": "ja",
            "media_attachments": [],
            "poll": null
        }),
        "en",
        "cfwdon-placeholder",
    );

    assert_eq!(
        document.pointer("/language"),
        Some(&serde_json::json!("en"))
    );
    assert_eq!(
        document.pointer("/detected_source_language"),
        Some(&serde_json::json!("ja"))
    );
}

#[test]
fn translation_document_uses_provider_display_name() {
    let document = build_translation_document_for_language(
        &serde_json::json!({
            "content": "<p>Hello</p>",
            "spoiler_text": "",
            "language": "en",
            "media_attachments": [],
            "poll": null
        }),
        "de",
        "DeepL.com",
    );

    assert_eq!(
        document.pointer("/provider"),
        Some(&serde_json::json!("DeepL.com"))
    );
}

#[test]
fn translation_provider_language_code_normalizes_target_codes() {
    assert_eq!(translation_provider_language_code("pt-BR"), "pt");
    assert_eq!(translation_provider_language_code("EN_us"), "en");
    assert_eq!(translation_provider_language_code("und"), "auto");
    assert_eq!(translation_provider_language_code(""), "auto");
}

#[test]
fn translation_provider_supported_target_language_prefers_primary_subtag() {
    let document = serde_json::json!({
        "en": ["pt", "de"],
        "ja": ["en", "pt"]
    });

    assert_eq!(
        translation_provider_supported_target_language(&document, "en-US", "de-DE"),
        Some("de".to_owned())
    );
    assert_eq!(
        translation_provider_supported_target_language(&document, "en-US", "pt-BR"),
        Some("pt".to_owned())
    );
    assert_eq!(
        translation_provider_supported_target_language(&document, "ja", "en-GB"),
        Some("en".to_owned())
    );
    assert_eq!(
        translation_provider_supported_target_language(&document, "en", "it"),
        None
    );
}

#[test]
fn translation_languages_document_flattens_supported_targets() {
    let document = build_translation_languages_document(&[
        TranslationProviderLanguageRow {
            code: Some("en".to_owned()),
            targets: Some(vec!["de".to_owned(), "es".to_owned()]),
        },
        TranslationProviderLanguageRow {
            code: Some("fr".to_owned()),
            targets: Some(vec!["de".to_owned()]),
        },
    ]);

    assert_eq!(
        document.pointer("/en"),
        Some(&serde_json::json!(["de", "es"]))
    );
    assert_eq!(document.pointer("/fr"), Some(&serde_json::json!(["de"])));
    assert_eq!(
        document.pointer("/und"),
        Some(&serde_json::json!(["de", "es"]))
    );
}

#[test]
fn deepl_translation_languages_document_exposes_all_targets_except_self() {
    let document = build_deepl_translation_languages_document(
        &["en".to_owned(), "ja".to_owned()],
        &["en".to_owned(), "ja".to_owned(), "de".to_owned()],
    );

    assert_eq!(
        document.pointer("/en"),
        Some(&serde_json::json!(["pt", "ja", "de"]))
    );
    assert_eq!(
        document.pointer("/ja"),
        Some(&serde_json::json!(["en", "pt", "de"]))
    );
    assert_eq!(
        document.pointer("/und"),
        Some(&serde_json::json!(["en", "pt", "ja", "de"]))
    );
}

#[test]
fn translation_language_pair_support_uses_source_or_auto_detection() {
    let document = serde_json::json!({
        "en": ["de", "es"],
        "fr": ["de"],
        "und": ["de", "es"]
    });

    assert!(translation_provider_supported_target_language(&document, "en", "de").is_some());
    assert!(translation_provider_supported_target_language(&document, "en-US", "de-DE").is_some());
    assert!(translation_provider_supported_target_language(&document, "und", "es").is_some());
    assert!(!translation_provider_supported_target_language(&document, "fr", "es").is_some());
}

#[test]
fn normalize_quote_approval_policy_accepts_supported_values() {
    assert_eq!(
        normalize_quote_approval_policy(Some(" followers ".to_owned())).unwrap(),
        Some(cfwdon_domain::QuoteApprovalPolicy::Followers)
    );
    assert_eq!(normalize_quote_approval_policy(None).unwrap(), None);
}

#[test]
fn normalize_quote_approval_policy_rejects_unknown_values() {
    let error = normalize_quote_approval_policy(Some("friends".to_owned())).unwrap_err();
    assert!(error.contains("quote_approval_policy"));
}

#[test]
fn normalized_action_uri_decodes_and_trims_query_values() {
    assert_eq!(
        normalized_action_uri(Some(
            "  https%3A%2F%2Fremote.example%2Fusers%2Falice%2Fstatuses%2F42  "
        )),
        Some("https://remote.example/users/alice/statuses/42".to_owned())
    );
}

#[test]
fn normalized_action_uri_rejects_empty_values() {
    assert_eq!(normalized_action_uri(Some("   ")), None);
    assert_eq!(normalized_action_uri(None), None);
}

#[test]
fn translation_target_language_prefers_request_then_viewer_then_instance() {
    let instance_languages = vec!["ja".to_owned(), "en".to_owned()];
    assert_eq!(
        translation_target_language(Some("fr"), Some("de"), &instance_languages, "es"),
        "fr"
    );
    assert_eq!(
        translation_target_language(None, Some("de"), &instance_languages, "es"),
        "de"
    );
    assert_eq!(
        translation_target_language(None, None, &instance_languages, "es"),
        "ja"
    );
    assert_eq!(
        translation_target_language(None, None, &Vec::new(), "es"),
        "es"
    );
}

#[test]
fn effective_local_quote_approval_defaults_to_public() {
    let status = LocalStatus {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        in_reply_to_account_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        text: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: cfwdon_domain::Visibility::Public,
        sensitive: false,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: cfwdon_domain::QuoteState::Accepted,
        application_id: None,
        card_json: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        updated_at: None,
    };

    assert_eq!(effective_local_quote_approval_policy(&status), "public");
}

#[test]
fn effective_local_quote_approval_forces_private_status_to_nobody() {
    let status = LocalStatus {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        in_reply_to_account_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        text: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: cfwdon_domain::Visibility::FollowersOnly,
        sensitive: false,
        language: Some("en".to_owned()),
        quote_approval_policy: Some(cfwdon_domain::QuoteApprovalPolicy::Public),
        quote_state: cfwdon_domain::QuoteState::Accepted,
        application_id: None,
        card_json: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        updated_at: None,
    };

    assert_eq!(effective_local_quote_approval_policy(&status), "nobody");
}

#[test]
fn local_quote_policy_allows_matches_policy_rules() {
    assert!(local_quote_policy_allows("public", false, false));
    assert!(local_quote_policy_allows("followers", false, true));
    assert!(!local_quote_policy_allows("followers", false, false));
    assert!(!local_quote_policy_allows("nobody", false, true));
    assert!(local_quote_policy_allows("nobody", true, false));
}

#[test]
fn effective_status_quote_state_defaults_to_accepted_without_quote() {
    let status = LocalStatus {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        in_reply_to_account_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        text: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: cfwdon_domain::Visibility::Public,
        sensitive: false,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: cfwdon_domain::QuoteState::Revoked,
        application_id: None,
        card_json: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        updated_at: None,
    };

    assert_eq!(effective_status_quote_state(&status), "accepted");
    assert!(!status_has_active_quote(&status));
}

#[test]
fn status_has_active_quote_depends_on_quote_state() {
    let mut status = LocalStatus {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        in_reply_to_account_id: None,
        boost_of_uri: None,
        quote_of_uri: Some("https://remote.example/@bob/1".to_owned()),
        content_html: "<p>hello</p>".to_owned(),
        text: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: cfwdon_domain::Visibility::Public,
        sensitive: false,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: cfwdon_domain::QuoteState::Pending,
        application_id: None,
        card_json: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        updated_at: None,
    };

    assert_eq!(effective_status_quote_state(&status), "pending");
    assert!(status_has_active_quote(&status));

    status.quote_state = cfwdon_domain::QuoteState::Revoked;
    assert_eq!(effective_status_quote_state(&status), "revoked");
    assert!(!status_has_active_quote(&status));
}

#[test]
fn local_status_allows_viewer_opens_direct_to_participants_only() {
    use cfwdon_domain::Visibility;

    assert!(local_status_allows_viewer(
        Visibility::Public,
        false,
        false,
        false
    ));
    assert!(local_status_allows_viewer(
        Visibility::Unlisted,
        false,
        false,
        false
    ));
    assert!(!local_status_allows_viewer(
        Visibility::FollowersOnly,
        false,
        false,
        false
    ));
    assert!(local_status_allows_viewer(
        Visibility::FollowersOnly,
        false,
        true,
        false
    ));
    assert!(local_status_allows_viewer(
        Visibility::FollowersOnly,
        true,
        false,
        false
    ));
    assert!(!local_status_allows_viewer(
        Visibility::Direct,
        false,
        true,
        false
    ));
    assert!(local_status_allows_viewer(
        Visibility::Direct,
        true,
        false,
        false
    ));
    assert!(local_status_allows_viewer(
        Visibility::Direct,
        false,
        false,
        true
    ));
}

#[test]
fn local_quote_revoke_allowed_requires_quote_author_and_active_quote() {
    let target_uri = "https://social.example/users/bob/statuses/target-1";
    let mut quote = LocalStatus {
        id: "quote-1".to_owned(),
        account_id: "alice".to_owned(),
        ap_id: Some("https://social.example/users/alice/statuses/quote-1".to_owned()),
        in_reply_to_id: None,
        in_reply_to_account_id: None,
        boost_of_uri: None,
        quote_of_uri: Some(target_uri.to_owned()),
        content_html: "<p>hello</p>".to_owned(),
        text: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: cfwdon_domain::Visibility::Public,
        sensitive: false,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: cfwdon_domain::QuoteState::Accepted,
        application_id: None,
        card_json: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        updated_at: None,
    };

    assert!(local_quote_revoke_allowed("alice", &quote, target_uri));
    assert!(!local_quote_revoke_allowed("bob", &quote, target_uri));
    assert!(!local_quote_revoke_allowed(
        "alice",
        &quote,
        "https://other.example/statuses/1"
    ));

    quote.quote_state = cfwdon_domain::QuoteState::Revoked;
    assert!(!local_quote_revoke_allowed("alice", &quote, target_uri));
}

#[test]
fn quote_document_with_state_wraps_status_payload() {
    let document = quote_document_with_state(
        "accepted",
        serde_json::json!({
            "id": "status-1"
        }),
    );

    assert_eq!(document["state"], serde_json::json!("accepted"));
    assert_eq!(
        document["quoted_status"]["id"],
        serde_json::json!("status-1")
    );
}

#[test]
fn pending_quote_document_uses_placeholder_shape() {
    let document = pending_quote_document();

    assert_eq!(document["state"], serde_json::json!("pending"));
    assert!(document["quoted_status"].is_null());
}

#[test]
fn quote_placeholder_document_preserves_requested_state() {
    let document = quote_placeholder_document("revoked");

    assert_eq!(document["state"], serde_json::json!("revoked"));
    assert!(document["quoted_status"].is_null());
}

#[test]
fn account_status_query_strings_populate_pagination_fields() {
    let query: AccountStatusesQuery =
        serde_urlencoded::from_str("limit=10&max_id=old&since_id=new&min_id=fresh").unwrap();

    assert_eq!(query.limit, Some(10));
    assert_eq!(query.max_id.as_deref(), Some("old"));
    assert_eq!(query.since_id.as_deref(), Some("new"));
    assert_eq!(query.min_id.as_deref(), Some("fresh"));
}

#[test]
fn normalize_status_history_entry_keeps_only_history_fields() {
    let value = serde_json::json!({
        "id": "status-1",
        "content": "<p>v2</p>",
        "spoiler_text": "cw",
        "sensitive": true,
        "created_at": "2026-04-18T00:00:00.000Z",
        "account": { "id": "acct-1" },
        "media_attachments": [{ "id": "media-1" }],
        "emojis": [],
        "poll": { "id": "poll-1" },
        "quote": null,
        "visibility": "public"
    });

    let normalized = normalize_status_history_entry(value);
    let object = normalized.as_object().unwrap();

    assert_eq!(object.len(), 9);
    assert_eq!(normalized["content"], "<p>v2</p>");
    assert_eq!(normalized["spoiler_text"], "cw");
    assert_eq!(normalized["sensitive"], true);
    assert_eq!(normalized["created_at"], "2026-04-18T00:00:00.000Z");
    assert!(normalized.get("id").is_none());
    assert!(normalized.get("visibility").is_none());
}

#[test]
fn normalize_status_history_entry_defaults_missing_optional_fields() {
    let normalized = normalize_status_history_entry(serde_json::json!({
        "content": "<p>v1</p>",
        "account": { "id": "acct-1" }
    }));

    assert_eq!(normalized["spoiler_text"], "");
    assert_eq!(normalized["sensitive"], false);
    assert_eq!(normalized["created_at"], "");
    assert_eq!(normalized["media_attachments"], serde_json::json!([]));
    assert_eq!(normalized["emojis"], serde_json::json!([]));
    assert!(normalized["poll"].is_null());
    assert!(normalized["quote"].is_null());
}

#[test]
fn first_url_from_text_trims_wrapping_punctuation() {
    assert_eq!(
        first_url_from_text("see (https://example.com/path), next").as_deref(),
        Some("https://example.com/path")
    );
    assert_eq!(first_url_from_text("no links here"), None);
}

#[test]
fn build_status_card_value_returns_mastodon_compatible_link_shape() {
    let card = build_status_card_value("hello https://example.com/article").unwrap();

    assert_eq!(card["type"], "link");
    assert_eq!(card["url"], "https://example.com/article");
    assert_eq!(card["provider_name"], "example.com");
    assert_eq!(card["provider_url"], "https://example.com");
    assert_eq!(card["title"], "article");
    assert_eq!(card["description"], "hello");
}

#[test]
fn build_status_card_value_derives_slug_title_and_provider_from_url() {
    let card = build_status_card_value(
        "Read this https://www.example.com/posts/hello-world.html?utm_source=test soon",
    )
    .unwrap();

    assert_eq!(card["provider_name"], "example.com");
    assert_eq!(card["provider_url"], "https://www.example.com");
    assert_eq!(card["title"], "hello world");
    assert_eq!(card["description"], "Read this soon");
}

#[test]
fn build_status_card_value_truncates_long_descriptions() {
    let long_prefix = "a".repeat(320);
    let card = build_status_card_value(&format!("{long_prefix} https://example.com/post")).unwrap();

    let description = card["description"].as_str().unwrap();
    assert!(description.ends_with('…'));
    assert!(description.chars().count() <= 301);
}

#[test]
fn extract_html_preview_metadata_reads_open_graph_fields() {
    let metadata = extract_html_preview_metadata(
        r#"
        <html><head>
        <meta property="og:title" content="Rust article">
        <meta property="og:description" content="A deep dive">
        <meta property="og:site_name" content="Example News">
        <meta property="og:image" content="https://cdn.example/cover.png">
        <meta property="og:image:width" content="1200">
        <meta property="og:image:height" content="630">
        </head></html>
        "#,
    );

    assert_eq!(metadata.title.as_deref(), Some("Rust article"));
    assert_eq!(metadata.description.as_deref(), Some("A deep dive"));
    assert_eq!(metadata.provider_name.as_deref(), Some("Example News"));
    assert_eq!(
        metadata.image.as_deref(),
        Some("https://cdn.example/cover.png")
    );
    assert_eq!(metadata.width, Some(1200));
    assert_eq!(metadata.height, Some(630));
}

#[test]
fn apply_html_preview_metadata_overwrites_basic_card_fields() {
    let mut card = build_status_card_value("see https://example.com/post").unwrap();
    let metadata = extract_html_preview_metadata(
        r#"
        <html>
          <head>
            <title>Ignored title</title>
            <meta name="description" content="Summary here">
            <meta property="og:title" content="Actual preview title">
            <meta property="og:site_name" content="Example Publication">
            <link rel="image_src" href="https://cdn.example/preview.jpg">
          </head>
        </html>
        "#,
    );

    apply_html_preview_metadata(&mut card, &metadata);

    assert_eq!(card["title"], "Actual preview title");
    assert_eq!(card["description"], "Summary here");
    assert_eq!(card["provider_name"], "Example Publication");
    assert_eq!(card["image"], "https://cdn.example/preview.jpg");
}
