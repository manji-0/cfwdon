use base64::Engine;

use super::{
    AUTH_CONTEXT_LIMIT, AccountRegistrationValidation, CreateStatusPollRequest,
    MastodonAccountResponse, MastodonReportResponse, NotificationEntry, NotificationsQuery,
    RemoteActorRow, RemotePollDraft, RemotePollOptionDraft, RemoteStatusPollOptionRow,
    RemoteStatusPollRow, RemoteStatusPollVoteRow, RemoteStatusRow, SearchCategoryFlags,
    SearchUrlQueryMode, SearchV2Query, StatusPollOptionRow, StatusPollRow, StatusRow,
    StreamingChannelValidationError, TagSearchMetrics, TagTimelineQuery, TimelinePaginationQuery,
    TranslationProviderLanguageRow, account_matches_search_terms, account_relationship_rank,
    account_search_is_complete_handle, account_search_non_exact_limit, account_search_rank,
    account_search_sort_key, account_search_term, account_search_terms,
    activitypub_profile_attachments, apply_activitypub_poll_fields, apply_html_preview_metadata,
    build_activitypub_actor_document, build_activitypub_delete_with_published_at,
    build_add_featured_activity_with_id, build_announcements_document,
    build_app_verify_credentials_document, build_app_verify_credentials_document_from_parts,
    build_deepl_request_body, build_deepl_translation_languages_document,
    build_delete_quote_authorization_activity, build_donation_campaign_document,
    build_email_confirmation_html, build_email_confirmation_subject, build_email_confirmation_text,
    build_email_confirmation_url, build_instance_v1_document, build_instance_v2_document,
    build_internal_cursor_link_for_url, build_internal_cursor_link_for_url_with_min_id,
    build_libretranslate_request_payload, build_nodeinfo_document, build_nodeinfo_links_document,
    build_notifications_v2_document, build_oauth_authorization_server_document,
    build_oauth_token_document, build_oauth_userinfo_document, build_poll_vote_activity_with_ids,
    build_remote_status_card_value, build_remove_featured_activity_with_id,
    build_status_card_value, build_status_update_activity_with_id,
    build_timeline_link_header_for_url, build_translation_document,
    build_translation_document_for_language, build_translation_languages_document,
    build_update_person_activity_with_id, classify_media_kind, configured_html_document,
    context_async_refresh_id, delivery_retry_delay_modifier, derive_link_timeline_match_urls,
    describe_outbound_activity, directory_order, effective_local_quote_approval_policy,
    effective_remote_status_quote_state, effective_search_v2_following, effective_search_v2_offset,
    effective_status_quote_state, extract_account_handles_from_text, extract_hashtags_from_html,
    extract_hashtags_from_text, extract_html_preview_metadata, extract_inbox_target_username,
    extract_mentions_from_text, extract_remote_note_object, extract_remote_poll_draft,
    extract_remote_profile_media_url, filter_notification_entries_by_query, first_url_from_text,
    follow_targets_local_actor, format_async_refresh_header_value, include_local_source,
    include_remote_source, initial_local_quote_approval_policy, instance_base_url,
    is_activitypub_actor_type, is_admin_account, is_follow_undo, local_quote_policy_allows,
    local_username_from_actor_uri, local_username_from_status_uri, mastodon_account_fields,
    matches_tag_timeline_filters, media_fallback_url, media_kind_label, media_object_url,
    nodeinfo_url, normalize_quote_approval_policy, normalize_scheduled_at,
    normalize_search_match_text, normalize_search_query_input, normalize_status_history_entry,
    normalize_status_poll, normalized_account_search_query, normalized_action_uri,
    notification_sort_key, notification_timestamp_sort_token,
    oauth_access_token_has_any_scope_json, object_attributed_to_remote_actor,
    optimistic_remote_poll_vote_deltas, outbound_terminal_failure_follow_state,
    paginate_tag_search_matches, parse_basic_authorization_header,
    parse_bearer_authorization_header, parse_csv_list, parse_deepl_translated_text,
    parse_http_url_parts, parse_internal_pagination_id, parse_libretranslate_translated_text,
    parse_lookup_handle, parse_media_focus, parse_remote_actor_profile_document,
    parse_status_search_query, parse_webfinger_resource, peer_authority_from_uri,
    pending_quote_document, quote_document_with_state, quote_placeholder_document,
    quote_target_uri_from_object, remap_remote_poll_vote_positions, remote_account_rest_id,
    remote_actor_uri_from_rest_id, remote_poll_draft_acknowledges_local_snapshot,
    remote_poll_draft_acknowledges_vote, remote_poll_should_refresh,
    remote_quote_state_for_local_target, remote_status_has_active_quote,
    remote_status_targets_local_viewer, remote_status_targets_local_viewer_account,
    remote_status_targets_local_viewer_followers, resolve_search_tag_name,
    scheduled_status_document, scheduled_status_document_with_params, search_category_flags,
    search_text_match_rank, search_v2_limit, search_v2_requires_auth,
    search_v2_type_allows_url_resource, search_v2_unauthenticated_error, search_v2_url_query_mode,
    set_instance_translation_enabled, status_has_active_quote, status_is_searchable_by_scope,
    status_matches_search_metadata, status_matches_search_scope, status_matches_search_syntax,
    status_matches_search_timestamp, status_search_query_terms, status_search_rank,
    streaming_channel_requires_auth, tag_matches_search_query, tag_search_rank,
    tag_search_sort_key, text_mentions_search_library_viewer, timeline_fetch_limit, timeline_limit,
    translation_cache_source_fingerprint, translation_provider_language_code,
    translation_provider_language_matches, translation_provider_supported_target_language,
    translation_target_language, trim_context_ancestors, trim_context_descendants,
    validate_account_registration_request, validate_poll_vote_submission,
    validate_scheduled_at_minimum_offset, validate_streaming_channel_request,
    visibility_from_activitypub_object,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    InstanceCapabilities, InstanceSummary, LocalAccount, ProfileField, SoftwareInfo, StatusDraft,
    Visibility,
};
use std::collections::{HashMap, HashSet};
use url::Url;

fn actor_fixture_account() -> LocalAccount {
    LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: "<p>Hello</p>".to_owned(),
        bio_text: "Hello".to_owned(),
        fields: vec![ProfileField {
            name: "Website".to_owned(),
            value: "https://example.com".to_owned(),
        }],
        locked: false,
        bot: false,
        discoverable: true,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("ja".to_owned()),
        avatar_object_key: Some("media/account/avatar/alice".to_owned()),
        avatar_content_type: Some("image/png".to_owned()),
        header_object_key: Some("media/account/header/alice".to_owned()),
        header_content_type: Some("image/jpeg".to_owned()),
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    }
}

#[test]
fn parse_webfinger_resource_extracts_local_handle() {
    let handle = parse_webfinger_resource("acct:alice@example.com").unwrap();
    assert_eq!(handle.username, "alice");
    assert_eq!(handle.domain.as_deref(), Some("example.com"));
}

#[test]
fn parse_webfinger_resource_accepts_case_insensitive_acct_scheme() {
    let handle = parse_webfinger_resource("ACCT:Alice@Example.Com").unwrap();
    assert_eq!(handle.username, "alice");
    assert_eq!(handle.domain.as_deref(), Some("example.com"));
}

#[test]
fn parse_webfinger_resource_rejects_non_acct_scheme() {
    let error = parse_webfinger_resource("https://example.com/users/alice").unwrap_err();
    assert!(error.to_string().contains("acct"));
}

#[test]
fn oauth_authorization_server_document_matches_mastodon_discovery_shape() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let document = build_oauth_authorization_server_document(&config);

    assert_eq!(
        document.pointer("/issuer"),
        Some(&serde_json::json!("https://social.example/"))
    );
    assert_eq!(
        document.pointer("/app_registration_endpoint"),
        Some(&serde_json::json!("https://social.example/api/v1/apps"))
    );
    assert_eq!(
        document.pointer("/response_modes_supported/2"),
        Some(&serde_json::json!("form_post"))
    );
    assert_eq!(
        document.pointer("/code_challenge_methods_supported/0"),
        Some(&serde_json::json!("S256"))
    );
    assert_eq!(
        document.pointer("/service_documentation"),
        Some(&serde_json::json!("https://docs.joinmastodon.org/"))
    );
}

#[test]
fn oauth_userinfo_document_exposes_standard_claims() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.media_public_base_url = Some("https://media.example.com".to_owned());
    let account = actor_fixture_account();
    let document = build_oauth_userinfo_document(&config, &account);

    assert_eq!(
        document.pointer("/iss"),
        Some(&serde_json::json!("https://social.example/"))
    );
    assert_eq!(
        document.pointer("/sub"),
        Some(&serde_json::json!("https://social.example/users/alice"))
    );
    assert_eq!(
        document.pointer("/preferred_username"),
        Some(&serde_json::json!("alice"))
    );
    assert_eq!(
        document.pointer("/profile"),
        Some(&serde_json::json!("https://social.example/@alice"))
    );
    assert_eq!(
        document.pointer("/picture"),
        Some(&serde_json::json!(
            "https://media.example.com/media/account/avatar/alice"
        ))
    );
}

#[test]
fn donation_campaign_document_uses_configured_upstream_shape() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.donation_campaign_json = Some(
        serde_json::json!({
            "id": "campaign-1",
            "banner_message": "Hi",
            "banner_button_text": "Donate!",
            "donation_message": "Hi!",
            "donation_button_text": "Money",
            "donation_success_post": "Success post",
            "amounts": {
                "one_time": {
                    "EUR": [1, 2, 3],
                    "USD": [4, 5, 6],
                },
                "monthly": {
                    "EUR": [1],
                    "USD": [2],
                },
            },
            "default_currency": "EUR",
            "donation_url": "https://sponsor.joinmastodon.org/donate/new",
            "locale": "en",
        })
        .to_string(),
    );

    let document = build_donation_campaign_document(&config).unwrap();

    assert_eq!(
        document.pointer("/id"),
        Some(&serde_json::json!("campaign-1"))
    );
    assert_eq!(
        document.pointer("/amounts/one_time/USD/2"),
        Some(&serde_json::json!(6))
    );
    assert_eq!(
        document.pointer("/donation_url"),
        Some(&serde_json::json!(
            "https://sponsor.joinmastodon.org/donate/new"
        ))
    );
}

#[test]
fn announcements_document_applies_read_and_reaction_state() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.announcements_json = Some(
        serde_json::json!([
            {
                "id": "announcement-1",
                "content": "<p>Hello</p>",
                "starts_at": serde_json::Value::Null,
                "ends_at": serde_json::Value::Null,
                "all_day": false,
                "published_at": "2026-04-20T00:00:00Z",
                "updated_at": serde_json::Value::Null,
                "mentions": [],
                "statuses": [],
                "tags": [],
                "emojis": [],
                "reactions": [
                    {
                        "name": "thumbsup",
                        "count": 0,
                        "me": false
                    }
                ]
            }
        ])
        .to_string(),
    );
    let read_ids = HashSet::from(["announcement-1".to_owned()]);
    let reaction_state = HashMap::from([(
        ("announcement-1".to_owned(), "thumbsup".to_owned()),
        (3, true),
    )]);

    let document = build_announcements_document(&config, &read_ids, &reaction_state);

    assert_eq!(document.len(), 1);
    assert_eq!(document[0].pointer("/read"), Some(&serde_json::json!(true)));
    assert_eq!(
        document[0].pointer("/reactions/0/count"),
        Some(&serde_json::json!(3))
    );
    assert_eq!(
        document[0].pointer("/reactions/0/me"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn app_verify_credentials_document_matches_mastodon_shape_without_secrets() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let document = build_app_verify_credentials_document(&config);

    assert_eq!(document.pointer("/id"), Some(&serde_json::json!("0")));
    assert_eq!(
        document.pointer("/name"),
        Some(&serde_json::json!("cfwdon"))
    );
    assert_eq!(
        document.pointer("/scopes/0"),
        Some(&serde_json::json!("read"))
    );
    assert_eq!(
        document.pointer("/redirect_uris/0"),
        Some(&serde_json::json!("urn:ietf:wg:oauth:2.0:oob"))
    );
    assert_eq!(document.pointer("/client_id"), None);
    assert_eq!(document.pointer("/client_secret"), None);
}

#[test]
fn app_verify_credentials_document_uses_configured_vapid_key() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.web_push_vapid_public_key = Some("BExamplePublicKey".to_owned());

    let document = build_app_verify_credentials_document(&config);

    assert_eq!(
        document.pointer("/vapid_key"),
        Some(&serde_json::json!("BExamplePublicKey"))
    );
}

#[test]
fn app_verify_credentials_document_from_parts_omits_client_secrets() {
    let document = build_app_verify_credentials_document_from_parts(
        "42",
        "Test Application",
        Some("https://app.example"),
        &[String::from("read"), String::from("write")],
        &[
            String::from("https://app.example/callback"),
            String::from("https://app.example/register"),
        ],
        "https://app.example/callback\nhttps://app.example/register",
        "BExamplePublicKey",
    );

    assert_eq!(document.pointer("/id"), Some(&serde_json::json!("42")));
    assert_eq!(
        document.pointer("/website"),
        Some(&serde_json::json!("https://app.example"))
    );
    assert_eq!(
        document.pointer("/scopes/1"),
        Some(&serde_json::json!("write"))
    );
    assert_eq!(
        document.pointer("/redirect_uris/1"),
        Some(&serde_json::json!("https://app.example/register"))
    );
    assert_eq!(document.pointer("/client_id"), None);
    assert_eq!(document.pointer("/client_secret"), None);
}

#[test]
fn parse_bearer_authorization_header_extracts_bearer_value() {
    assert_eq!(
        parse_bearer_authorization_header("Bearer secret-token"),
        Some("secret-token".to_owned())
    );
}

#[test]
fn parse_bearer_authorization_header_rejects_non_bearer_header() {
    assert_eq!(parse_bearer_authorization_header("Basic abc123"), None);
}

#[test]
fn parse_basic_authorization_header_extracts_client_credentials() {
    let header = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("client-id:client-secret")
    );

    assert_eq!(
        parse_basic_authorization_header(&header),
        Some(("client-id".to_owned(), "client-secret".to_owned()))
    );
}

#[test]
fn oauth_token_document_matches_upstream_shape() {
    let document = build_oauth_token_document("token-1", "read write");

    assert_eq!(
        document.pointer("/access_token"),
        Some(&serde_json::json!("token-1"))
    );
    assert_eq!(
        document.pointer("/token_type"),
        Some(&serde_json::json!("Bearer"))
    );
    assert_eq!(
        document.pointer("/scope"),
        Some(&serde_json::json!("read write"))
    );
    assert!(
        document
            .pointer("/created_at")
            .and_then(serde_json::Value::as_i64)
            .is_some()
    );
}

#[test]
fn scheduled_status_document_matches_upstream_shape() {
    let document = scheduled_status_document("sched-1");

    assert_eq!(document.pointer("/id"), Some(&serde_json::json!("sched-1")));
    assert_eq!(
        document.pointer("/scheduled_at"),
        Some(&serde_json::json!("2099-01-01T00:00:00.000Z"))
    );
    assert_eq!(
        document.pointer("/params/poll"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(
        document.pointer("/params/text"),
        Some(&serde_json::json!(""))
    );
    assert_eq!(
        document.pointer("/params/application_id"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        document.pointer("/params/with_rate_limit"),
        Some(&serde_json::json!(false))
    );
}

#[test]
fn scheduled_status_document_with_params_reflects_draft_values() {
    let draft = StatusDraft {
        text: "scheduled hello".to_owned(),
        visibility: Visibility::Unlisted,
        spoiler_text: "cw".to_owned(),
        sensitive: true,
        language: Some("ja".to_owned()),
        quote_approval_policy: None,
        in_reply_to_id: Some("status-1".to_owned()),
        media_ids: vec!["media-1".to_owned()],
        poll: None,
    };
    let document =
        scheduled_status_document_with_params("sched-2", "2099-02-03T04:05:06Z", Some(&draft));

    assert_eq!(
        document.pointer("/scheduled_at"),
        Some(&serde_json::json!("2099-02-03T04:05:06Z"))
    );
    assert_eq!(
        document.pointer("/params/text"),
        Some(&serde_json::json!("scheduled hello"))
    );
    assert_eq!(
        document.pointer("/params/visibility"),
        Some(&serde_json::json!("unlisted"))
    );
    assert_eq!(
        document.pointer("/params/media_ids/0"),
        Some(&serde_json::json!("media-1"))
    );
    assert_eq!(
        document.pointer("/params/in_reply_to_id"),
        Some(&serde_json::json!("status-1"))
    );
}

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
fn libretranslate_request_payload_matches_provider_shape() {
    let payload = build_libretranslate_request_payload(
        "<p>Hello</p>",
        "en-US",
        "ja-JP",
        "html",
        Some("secret"),
    );

    assert_eq!(
        payload.pointer("/q"),
        Some(&serde_json::json!("<p>Hello</p>"))
    );
    assert_eq!(payload.pointer("/source"), Some(&serde_json::json!("en")));
    assert_eq!(payload.pointer("/target"), Some(&serde_json::json!("ja")));
    assert_eq!(payload.pointer("/format"), Some(&serde_json::json!("html")));
    assert_eq!(
        payload.pointer("/api_key"),
        Some(&serde_json::json!("secret"))
    );
}

#[test]
fn libretranslate_response_extracts_translated_text() {
    assert_eq!(
        parse_libretranslate_translated_text(&serde_json::json!({
            "translatedText": "<p>こんにちは</p>"
        })),
        Some("<p>こんにちは</p>".to_owned())
    );
    assert_eq!(
        parse_libretranslate_translated_text(&serde_json::json!({ "error": "missing" })),
        None
    );
}

#[test]
fn deepl_request_body_uses_uppercase_language_codes() {
    let body = build_deepl_request_body("<p>Hello</p>", "en-US", "ja-JP");

    assert!(body.contains("text=%3Cp%3EHello%3C%2Fp%3E"));
    assert!(body.contains("source_lang=EN-US"));
    assert!(body.contains("target_lang=ja-JP"));
    assert!(body.contains("tag_handling=html"));
}

#[test]
fn deepl_request_body_omits_unknown_source_language() {
    let body = build_deepl_request_body("<p>Hello</p>", "und", "ja");

    assert!(body.contains("text=%3Cp%3EHello%3C%2Fp%3E"));
    assert!(!body.contains("source_lang="));
    assert!(body.contains("target_lang=ja"));
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
fn deepl_response_extracts_translated_text() {
    assert_eq!(
        parse_deepl_translated_text(&serde_json::json!({
            "translations": [
                { "text": "<p>こんにちは</p>" }
            ]
        })),
        Some("<p>こんにちは</p>".to_owned())
    );
    assert_eq!(
        parse_deepl_translated_text(&serde_json::json!({ "translations": [] })),
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

    assert!(translation_provider_language_matches(&document, "en", "de"));
    assert!(translation_provider_language_matches(
        &document, "en-US", "de-DE"
    ));
    assert!(translation_provider_language_matches(
        &document, "und", "es"
    ));
    assert!(!translation_provider_language_matches(
        &document, "fr", "es"
    ));
}

#[test]
fn translation_cache_source_fingerprint_tracks_translatable_fields() {
    let base = serde_json::json!({
        "content": "<p>Hello</p>",
        "spoiler_text": "cw",
        "language": "en",
        "account": { "display_name": "Alice" },
        "media_attachments": [
            { "id": "media-1", "description": "alt text", "url": "https://media.example/1" }
        ],
        "poll": {
            "id": "poll-1",
            "options": [
                { "title": "One", "votes_count": 1 }
            ]
        }
    });
    let same_translatable_fields = serde_json::json!({
        "content": "<p>Hello</p>",
        "spoiler_text": "cw",
        "language": "en",
        "account": { "display_name": "Changed" },
        "media_attachments": [
            { "id": "media-1", "description": "alt text", "url": "https://media.example/changed" }
        ],
        "poll": {
            "id": "poll-1",
            "options": [
                { "title": "One", "votes_count": 99 }
            ]
        }
    });
    let edited_content = serde_json::json!({
        "content": "<p>Hello edited</p>",
        "spoiler_text": "cw",
        "language": "en",
        "media_attachments": [
            { "id": "media-1", "description": "alt text" }
        ],
        "poll": {
            "options": [
                { "title": "One" }
            ]
        }
    });

    assert_eq!(
        translation_cache_source_fingerprint(&base).unwrap(),
        translation_cache_source_fingerprint(&same_translatable_fields).unwrap()
    );
    assert_ne!(
        translation_cache_source_fingerprint(&base).unwrap(),
        translation_cache_source_fingerprint(&edited_content).unwrap()
    );
}

#[test]
fn normalize_quote_approval_policy_accepts_supported_values() {
    assert_eq!(
        normalize_quote_approval_policy(Some(" followers ".to_owned())).unwrap(),
        Some("followers".to_owned())
    );
    assert_eq!(normalize_quote_approval_policy(None).unwrap(), None);
}

#[test]
fn normalize_quote_approval_policy_rejects_unknown_values() {
    let error = normalize_quote_approval_policy(Some("friends".to_owned())).unwrap_err();
    assert!(error.contains("quote_approval_policy"));
}

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
fn build_notifications_v2_document_collects_accounts_statuses_and_groups() {
    let entries = vec![
        NotificationEntry {
            id: "mention-1".to_owned(),
            created_at: "2026-04-19T00:00:00Z".to_owned(),
            value: serde_json::json!({
                "id": "mention-1",
                "type": "mention",
                "group_key": "mention-1",
                "account": {"id": "alice@remote.example", "acct": "alice@remote.example"},
                "status": {"id": "status-1", "content": "<p>hello</p>"}
            }),
        },
        NotificationEntry {
            id: "follow-1".to_owned(),
            created_at: "2026-04-19T00:01:00Z".to_owned(),
            value: serde_json::json!({
                "id": "follow-1",
                "type": "follow",
                "group_key": "follow-1",
                "account": {"id": "alice@remote.example", "acct": "alice@remote.example"}
            }),
        },
    ];

    let document = build_notifications_v2_document(&entries);
    assert_eq!(document["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(document["statuses"].as_array().unwrap().len(), 1);
    assert_eq!(document["notification_groups"].as_array().unwrap().len(), 2);
    assert_eq!(document["notification_groups"][0]["type"], "mention");
    assert_eq!(document["notification_groups"][0]["notifications_count"], 1);
    assert_eq!(
        document["notification_groups"][0]["sample_account_ids"],
        serde_json::json!(["alice@remote.example"])
    );
    assert_eq!(document["notification_groups"][0]["status_id"], "status-1");
    assert_eq!(
        document["notification_groups"][1]["status_id"],
        serde_json::Value::Null
    );
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
fn describe_outbound_activity_extracts_id_and_type() {
    let descriptor = describe_outbound_activity(
        r#"{"id":"https://social.example/users/alice/likes/123","type":"Like"}"#,
    )
    .unwrap();

    assert_eq!(
        descriptor.activity_id,
        "https://social.example/users/alice/likes/123"
    );
    assert_eq!(descriptor.activity_type, "Like");
}

#[test]
fn describe_outbound_activity_rejects_missing_fields() {
    assert!(describe_outbound_activity(r#"{"type":"Like"}"#).is_err());
    assert!(describe_outbound_activity(r#"{"id":"abc"}"#).is_err());
}

#[test]
fn extract_remote_note_object_supports_note_question_and_create_wrappers() {
    let note = serde_json::json!({"type":"Note","id":"https://remote.example/notes/1"});
    assert_eq!(
        extract_remote_note_object(&note)
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("https://remote.example/notes/1")
    );

    let question = serde_json::json!({
        "type":"Question",
        "id":"https://remote.example/notes/3",
        "oneOf":[
            {"type":"Note","name":"yes","replies":{"totalItems":2}},
            {"type":"Note","name":"no","replies":{"totalItems":1}}
        ]
    });
    assert_eq!(
        extract_remote_note_object(&question)
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("https://remote.example/notes/3")
    );

    let create = serde_json::json!({
        "type":"Create",
        "object":{"type":"Question","id":"https://remote.example/notes/2"}
    });
    assert_eq!(
        extract_remote_note_object(&create)
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("https://remote.example/notes/2")
    );
}

#[test]
fn extract_remote_note_object_rejects_non_note_documents() {
    let actor = serde_json::json!({"type":"Person","id":"https://remote.example/users/alice"});
    assert!(extract_remote_note_object(&actor).is_none());
}

#[test]
fn object_attributed_to_remote_actor_accepts_matching_actor_and_fallback() {
    let activity = serde_json::json!({
        "actor": "https://remote.example/users/alice",
        "object": {
            "type": "Note",
            "id": "https://remote.example/users/alice/statuses/1",
            "attributedTo": "https://remote.example/users/alice"
        }
    });
    assert!(object_attributed_to_remote_actor(
        activity.get("object").unwrap(),
        &activity,
        "https://remote.example/users/alice",
    ));

    let fallback_activity = serde_json::json!({
        "actor": "https://remote.example/users/alice",
        "object": {
            "type": "Note",
            "id": "https://remote.example/users/alice/statuses/2"
        }
    });
    assert!(object_attributed_to_remote_actor(
        fallback_activity.get("object").unwrap(),
        &fallback_activity,
        "https://remote.example/users/alice",
    ));
}

#[test]
fn object_attributed_to_remote_actor_rejects_mismatch() {
    let activity = serde_json::json!({
        "actor": "https://remote.example/users/alice",
        "object": {
            "type": "Note",
            "id": "https://remote.example/users/alice/statuses/3",
            "attributedTo": "https://evil.example/users/mallory"
        }
    });
    assert!(!object_attributed_to_remote_actor(
        activity.get("object").unwrap(),
        &activity,
        "https://remote.example/users/alice",
    ));
}

#[test]
fn extract_remote_poll_draft_reads_question_options_and_counts() {
    let question = serde_json::json!({
        "type":"Question",
        "endTime":"2026-03-01T00:00:00Z",
        "votersCount": 2,
        "anyOf":[
            {"type":"Note","name":"rust","replies":{"totalItems":2}},
            {"type":"Note","name":"workers","replies":{"totalItems":1}}
        ]
    });

    let poll = extract_remote_poll_draft(&question).unwrap();
    assert!(poll.multiple);
    assert_eq!(poll.expires_at.as_deref(), Some("2026-03-01T00:00:00Z"));
    assert_eq!(poll.voters_count, Some(2));
    assert_eq!(poll.votes_count, 3);
    assert_eq!(poll.options.len(), 2);
    assert_eq!(poll.options[0].title, "rust");
    assert_eq!(poll.options[1].votes_count, 1);
}

#[test]
fn remap_remote_poll_vote_positions_prefers_matching_title_after_reorder() {
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "green".to_owned(),
            votes_count: 5,
        },
        RemoteStatusPollOptionRow {
            title: "orange".to_owned(),
            votes_count: 3,
        },
        RemoteStatusPollOptionRow {
            title: "blue".to_owned(),
            votes_count: 1,
        },
    ];
    let votes = vec![RemoteStatusPollVoteRow {
        option_position: 0,
        option_title: Some("orange".to_owned()),
    }];

    assert_eq!(remap_remote_poll_vote_positions(&options, &votes), vec![1]);
}

#[test]
fn remap_remote_poll_vote_positions_falls_back_to_stored_position_for_legacy_rows() {
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "yes".to_owned(),
            votes_count: 2,
        },
        RemoteStatusPollOptionRow {
            title: "no".to_owned(),
            votes_count: 1,
        },
    ];
    let votes = vec![RemoteStatusPollVoteRow {
        option_position: 1,
        option_title: None,
    }];

    assert_eq!(remap_remote_poll_vote_positions(&options, &votes), vec![1]);
}

#[test]
fn remap_remote_poll_vote_positions_drops_unresolvable_stale_votes() {
    let options = vec![RemoteStatusPollOptionRow {
        title: "green".to_owned(),
        votes_count: 2,
    }];
    let votes = vec![RemoteStatusPollVoteRow {
        option_position: 4,
        option_title: Some("orange".to_owned()),
    }];

    assert!(remap_remote_poll_vote_positions(&options, &votes).is_empty());
}

#[test]
fn optimistic_remote_poll_vote_deltas_increment_multi_voter_once() {
    assert_eq!(
        optimistic_remote_poll_vote_deltas(true, false, 2),
        (2, Some(1))
    );
    assert_eq!(optimistic_remote_poll_vote_deltas(true, true, 1), (1, None));
}

#[test]
fn optimistic_remote_poll_vote_deltas_do_not_set_single_choice_voters_count() {
    assert_eq!(
        optimistic_remote_poll_vote_deltas(false, false, 1),
        (1, None)
    );
}

#[test]
fn remote_poll_draft_acknowledges_vote_accepts_matching_or_newer_totals() {
    let poll = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 1,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(2),
        votes_count: 3,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "rust".to_owned(),
            votes_count: 2,
        },
        RemoteStatusPollOptionRow {
            title: "workers".to_owned(),
            votes_count: 1,
        },
    ];
    let fetched = RemotePollDraft {
        multiple: true,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(3),
        votes_count: 4,
        expired: false,
        options: vec![
            RemotePollOptionDraft {
                title: "rust".to_owned(),
                votes_count: 3,
            },
            RemotePollOptionDraft {
                title: "workers".to_owned(),
                votes_count: 1,
            },
        ],
    };

    assert!(remote_poll_draft_acknowledges_vote(
        &poll,
        &options,
        &fetched,
        false,
        &[0]
    ));
}

#[test]
fn remote_poll_draft_acknowledges_vote_rejects_stale_totals() {
    let poll = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 0,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(3),
        votes_count: 3,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "yes".to_owned(),
            votes_count: 2,
        },
        RemoteStatusPollOptionRow {
            title: "no".to_owned(),
            votes_count: 1,
        },
    ];
    let fetched = RemotePollDraft {
        multiple: false,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(3),
        votes_count: 3,
        expired: false,
        options: vec![
            RemotePollOptionDraft {
                title: "yes".to_owned(),
                votes_count: 2,
            },
            RemotePollOptionDraft {
                title: "no".to_owned(),
                votes_count: 1,
            },
        ],
    };

    assert!(!remote_poll_draft_acknowledges_vote(
        &poll,
        &options,
        &fetched,
        false,
        &[0]
    ));
}

#[test]
fn remote_poll_draft_acknowledges_local_snapshot_accepts_matching_or_newer_totals() {
    let poll = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 1,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(4),
        votes_count: 6,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "rust".to_owned(),
            votes_count: 4,
        },
        RemoteStatusPollOptionRow {
            title: "workers".to_owned(),
            votes_count: 2,
        },
    ];
    let fetched = RemotePollDraft {
        multiple: true,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(5),
        votes_count: 7,
        expired: false,
        options: vec![
            RemotePollOptionDraft {
                title: "rust".to_owned(),
                votes_count: 4,
            },
            RemotePollOptionDraft {
                title: "workers".to_owned(),
                votes_count: 3,
            },
        ],
    };

    assert!(remote_poll_draft_acknowledges_local_snapshot(
        &poll, &options, &fetched
    ));
}

#[test]
fn remote_poll_draft_acknowledges_local_snapshot_rejects_stale_option_totals() {
    let poll = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 0,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(3),
        votes_count: 3,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "yes".to_owned(),
            votes_count: 2,
        },
        RemoteStatusPollOptionRow {
            title: "no".to_owned(),
            votes_count: 1,
        },
    ];
    let fetched = RemotePollDraft {
        multiple: false,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(4),
        votes_count: 4,
        expired: false,
        options: vec![
            RemotePollOptionDraft {
                title: "yes".to_owned(),
                votes_count: 1,
            },
            RemotePollOptionDraft {
                title: "no".to_owned(),
                votes_count: 3,
            },
        ],
    };

    assert!(!remote_poll_draft_acknowledges_local_snapshot(
        &poll, &options, &fetched
    ));
}

#[test]
fn build_poll_vote_activity_uses_question_reply_shape() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    let (activity_id, payload) = build_poll_vote_activity_with_ids(
        &config,
        &account,
        "https://remote.example/users/bob",
        "https://remote.example/questions/1",
        "orange",
        "https://social.example/users/alice/votes/test-vote",
        "https://social.example/users/alice/votes/test-vote/activity",
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&payload).unwrap();
    assert_eq!(value["id"], serde_json::json!(activity_id));
    assert_eq!(value["type"], serde_json::json!("Create"));
    assert_eq!(
        value["to"],
        serde_json::json!(["https://remote.example/users/bob"])
    );
    assert_eq!(
        value["object"]["inReplyTo"],
        serde_json::json!("https://remote.example/questions/1")
    );
    assert_eq!(value["object"]["name"], serde_json::json!("orange"));
}

#[test]
fn build_status_update_activity_wraps_question_object() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
    let object = serde_json::json!({
        "id": "https://social.example/users/alice/statuses/status-1",
        "type": "Question",
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": ["https://social.example/users/alice/followers"],
    });

    let payload = build_status_update_activity_with_id(
        &config,
        &account,
        object,
        "https://social.example/users/alice/statuses/status-1/updates/test",
        "2026-02-01T00:00:00Z",
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&payload).unwrap();
    assert_eq!(value["type"], serde_json::json!("Update"));
    assert_eq!(
        value["id"],
        serde_json::json!("https://social.example/users/alice/statuses/status-1/updates/test")
    );
    assert_eq!(value["object"]["type"], serde_json::json!("Question"));
    assert_eq!(
        value["to"],
        serde_json::json!(["https://www.w3.org/ns/activitystreams#Public"])
    );
}

#[test]
fn build_featured_collection_activities_target_followers_collection() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: true,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: None,
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    let add = serde_json::from_str::<serde_json::Value>(
        &build_add_featured_activity_with_id(
            &config,
            &account,
            "https://social.example/users/alice/statuses/123",
            "https://social.example/users/alice/collections/featured/add/test",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(add["type"], "Add");
    assert_eq!(
        add["target"],
        "https://social.example/users/alice/collections/featured"
    );
    assert_eq!(
        add["to"],
        serde_json::json!(["https://social.example/users/alice/followers"])
    );
    assert_eq!(
        add["object"],
        "https://social.example/users/alice/statuses/123"
    );

    let remove = serde_json::from_str::<serde_json::Value>(
        &build_remove_featured_activity_with_id(
            &config,
            &account,
            "https://social.example/users/alice/statuses/123",
            "https://social.example/users/alice/collections/featured/remove/test",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(remove["type"], "Remove");
    assert_eq!(
        remove["target"],
        "https://social.example/users/alice/collections/featured"
    );
    assert_eq!(
        remove["to"],
        serde_json::json!(["https://social.example/users/alice/followers"])
    );
    assert_eq!(
        remove["object"],
        "https://social.example/users/alice/statuses/123"
    );
}

#[test]
fn apply_activitypub_poll_fields_uses_question_shape_for_single_choice() {
    let poll = StatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 0,
        hide_totals: 0,
        expires_at: "2026-02-01T00:00:00Z".to_owned(),
    };
    let options = vec![
        StatusPollOptionRow {
            title: "yes".to_owned(),
            votes_count: 2,
        },
        StatusPollOptionRow {
            title: "no".to_owned(),
            votes_count: 1,
        },
    ];
    let mut object = serde_json::json!({
        "type": "Note",
        "id": "https://social.example/users/alice/statuses/status-1",
    });

    apply_activitypub_poll_fields(&mut object, &poll, &options, 3, true);
    assert_eq!(object["type"], serde_json::json!("Question"));
    assert_eq!(object["endTime"], serde_json::json!("2026-02-01T00:00:00Z"));
    assert_eq!(object["closed"], serde_json::json!("2026-02-01T00:00:00Z"));
    assert_eq!(object["votersCount"], serde_json::json!(3));
    assert!(object.get("anyOf").is_none());
    assert_eq!(object["oneOf"][0]["name"], serde_json::json!("yes"));
    assert_eq!(
        object["oneOf"][1]["replies"]["totalItems"],
        serde_json::json!(1)
    );
}

#[test]
fn apply_activitypub_poll_fields_uses_any_of_for_multiple_choice() {
    let poll = StatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 1,
        hide_totals: 0,
        expires_at: "2026-02-01T00:00:00Z".to_owned(),
    };
    let options = vec![
        StatusPollOptionRow {
            title: "rust".to_owned(),
            votes_count: 2,
        },
        StatusPollOptionRow {
            title: "workers".to_owned(),
            votes_count: 3,
        },
    ];
    let mut object = serde_json::json!({
        "type": "Note",
        "id": "https://social.example/users/alice/statuses/status-1",
    });

    apply_activitypub_poll_fields(&mut object, &poll, &options, 4, false);
    assert_eq!(object["type"], serde_json::json!("Question"));
    assert!(object.get("oneOf").is_none());
    assert_eq!(object["anyOf"][0]["name"], serde_json::json!("rust"));
    assert_eq!(
        object["anyOf"][1]["replies"]["totalItems"],
        serde_json::json!(3)
    );
    assert!(object.get("closed").is_none());
}

#[test]
fn outbound_terminal_failure_marks_follow_as_failed_only_for_follow() {
    assert_eq!(
        outbound_terminal_failure_follow_state("Follow"),
        Some("failed")
    );
    assert_eq!(outbound_terminal_failure_follow_state("Undo"), None);
    assert_eq!(outbound_terminal_failure_follow_state("Like"), None);
}

#[test]
fn instance_base_url_normalizes_bare_domain() {
    let config = AppConfig::new("example.com", "cfwdon", "test instance");
    assert_eq!(instance_base_url(&config), "https://example.com");
}

#[test]
fn instance_base_url_preserves_explicit_scheme() {
    let config = AppConfig::new("https://social.example.com", "cfwdon", "test instance");
    assert_eq!(instance_base_url(&config), "https://social.example.com");
}

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

#[test]
fn parse_http_url_parts_keeps_path_and_query() {
    let (host, path) =
        parse_http_url_parts("https://remote.example/inbox/shared?foo=bar#ignored").unwrap();
    assert_eq!(host, "remote.example");
    assert_eq!(path, "/inbox/shared?foo=bar");
}

#[test]
fn parse_http_url_parts_adds_root_for_bare_query() {
    let (host, path) = parse_http_url_parts("https://remote.example?foo=bar").unwrap();
    assert_eq!(host, "remote.example");
    assert_eq!(path, "/?foo=bar");
}

#[test]
fn delivery_retry_delay_backoff_steps_up() {
    assert_eq!(delivery_retry_delay_modifier(1), "+1 minute");
    assert_eq!(delivery_retry_delay_modifier(2), "+5 minutes");
    assert_eq!(delivery_retry_delay_modifier(3), "+15 minutes");
    assert_eq!(delivery_retry_delay_modifier(4), "+60 minutes");
    assert_eq!(delivery_retry_delay_modifier(8), "+60 minutes");
}

#[test]
fn follow_targets_local_actor_accepts_string_and_object_forms() {
    assert!(follow_targets_local_actor(
        Some(&serde_json::json!("https://example.com/users/alice")),
        "https://example.com/users/alice",
    ));
    assert!(follow_targets_local_actor(
        Some(&serde_json::json!({"id": "https://example.com/users/alice"})),
        "https://example.com/users/alice",
    ));
    assert!(!follow_targets_local_actor(
        Some(&serde_json::json!("https://example.com/users/bob")),
        "https://example.com/users/alice",
    ));
}

#[test]
fn is_follow_undo_accepts_follow_object_for_same_actor() {
    assert!(is_follow_undo(
        Some(&serde_json::json!({
            "type": "Follow",
            "actor": "https://remote.example/users/bob",
        })),
        "https://remote.example/users/bob",
        "https://remote.example/@bob",
    ));
    assert!(!is_follow_undo(
        Some(&serde_json::json!({
            "type": "Like",
            "actor": "https://remote.example/users/bob",
        })),
        "https://remote.example/users/bob",
        "https://remote.example/@bob",
    ));
}

#[test]
fn extract_inbox_target_username_supports_follow_undo_accept_reject_and_create() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Follow",
                "object": "https://social.example/users/alice",
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Accept",
                "object": {
                    "type": "Follow",
                    "actor": "https://social.example/users/alice",
                    "object": "https://remote.example/users/bob"
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Reject",
                "object": {
                    "type": "Follow",
                    "actor": "https://social.example/users/alice",
                    "object": "https://remote.example/users/bob"
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Undo",
                "object": {
                    "type": "Follow",
                    "object": "https://social.example/users/alice",
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Create",
                "object": {
                    "to": ["https://social.example/users/alice"]
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Create",
                "object": {
                    "to": ["https://www.w3.org/ns/activitystreams#Public"],
                    "cc": ["https://social.example/users/alice/followers"]
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Update",
                "object": {
                    "to": ["https://www.w3.org/ns/activitystreams#Public"],
                    "cc": ["https://social.example/users/alice/followers"]
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Like",
                "object": "https://social.example/users/alice/statuses/status-1"
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Undo",
                "object": {
                    "type": "Create",
                    "object": {
                        "type": "Note",
                        "inReplyTo": "https://social.example/users/alice/statuses/status-1"
                    }
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Undo",
                "object": {
                    "type": "Announce",
                    "object": "https://social.example/users/alice/statuses/status-1"
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Announce",
                "object": {
                    "type": "Note",
                    "id": "https://remote.example/users/bob/statuses/quote-1",
                    "quoteUri": "https://social.example/users/alice/statuses/status-1"
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Announce",
                "object": {
                    "type": "Note",
                    "id": "https://remote.example/users/bob/statuses/quote-2",
                    "to": ["https://social.example/users/alice"]
                }
            })
        ),
        Some("alice".to_owned())
    );
}

#[test]
fn quote_target_uri_from_object_supports_fedibird_and_misskey_fields() {
    assert_eq!(
        quote_target_uri_from_object(&serde_json::json!({
            "quoteUri": "https://remote.example/statuses/1"
        })),
        Some("https://remote.example/statuses/1".to_owned())
    );
    assert_eq!(
        quote_target_uri_from_object(&serde_json::json!({
            "quoteUrl": "https://remote.example/statuses/2"
        })),
        Some("https://remote.example/statuses/2".to_owned())
    );
    assert_eq!(
        quote_target_uri_from_object(&serde_json::json!({
            "_misskey_quote": "https://remote.example/statuses/3"
        })),
        Some("https://remote.example/statuses/3".to_owned())
    );
}

#[test]
fn local_username_from_actor_uri_matches_local_users_only() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert_eq!(
        local_username_from_actor_uri(&config, "https://social.example/users/alice"),
        Some("alice".to_owned())
    );
    assert_eq!(
        local_username_from_actor_uri(&config, "https://remote.example/users/alice"),
        None
    );
    assert_eq!(
        local_username_from_actor_uri(&config, "https://social.example/@alice"),
        Some("alice".to_owned())
    );
}

#[test]
fn local_username_from_status_uri_matches_local_statuses_only() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert_eq!(
        local_username_from_status_uri(
            &config,
            "https://social.example/users/alice/statuses/status-1"
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        local_username_from_status_uri(
            &config,
            "https://remote.example/users/alice/statuses/status-1"
        ),
        None
    );
    assert_eq!(
        local_username_from_status_uri(&config, "https://social.example/@alice/status-1"),
        Some("alice".to_owned())
    );
    assert_eq!(
        local_username_from_status_uri(&config, "https://social.example/@alice/statuses/status-1"),
        Some("alice".to_owned())
    );
}

#[test]
fn visibility_from_activitypub_object_detects_public_and_unlisted() {
    assert_eq!(
        visibility_from_activitypub_object(&serde_json::json!({
            "to": ["https://www.w3.org/ns/activitystreams#Public"]
        })),
        "public"
    );
    assert_eq!(
        visibility_from_activitypub_object(&serde_json::json!({
            "cc": ["https://www.w3.org/ns/activitystreams#Public"]
        })),
        "unlisted"
    );
    assert_eq!(
        visibility_from_activitypub_object(&serde_json::json!({
            "to": ["https://social.example/users/alice/followers"]
        })),
        "private"
    );
}

#[test]
fn remote_account_rest_id_round_trips_actor_uri() {
    let actor_uri = "https://remote.example/users/alice";
    let id = remote_account_rest_id(actor_uri);
    assert_eq!(
        remote_actor_uri_from_rest_id(&id).as_deref(),
        Some(actor_uri)
    );
}

#[test]
fn parse_lookup_handle_defaults_bare_username_to_local_domain() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let handle = parse_lookup_handle("alice", &config).unwrap();
    assert_eq!(handle.username, "alice");
    assert_eq!(handle.domain.as_deref(), Some("social.example"));
}

#[test]
fn parse_lookup_handle_accepts_case_insensitive_acct_scheme() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let handle = parse_lookup_handle("ACCT:Alice@Remote.Example", &config).unwrap();
    assert_eq!(handle.username, "alice");
    assert_eq!(handle.domain.as_deref(), Some("remote.example"));
}

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
fn oauth_access_token_scopes_match_search_permissions() {
    let scopes_json = serde_json::to_string(&vec!["read".to_owned(), "write".to_owned()]).unwrap();

    assert!(oauth_access_token_has_any_scope_json(
        &scopes_json,
        &["read:search", "read"]
    ));
    assert!(!oauth_access_token_has_any_scope_json(
        &scopes_json,
        &["follow", "admin"]
    ));
}

#[test]
fn remote_poll_should_refresh_only_for_signed_in_active_polls() {
    let active = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 0,
        expires_at: None,
        voters_count: None,
        votes_count: 0,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let expired = RemoteStatusPollRow {
        id: "poll-2".to_owned(),
        status_id: "status-2".to_owned(),
        multiple: 0,
        expires_at: None,
        voters_count: None,
        votes_count: 0,
        expired: 1,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let viewer = actor_fixture_account();

    assert!(remote_poll_should_refresh(&active, Some(&viewer)));
    assert!(!remote_poll_should_refresh(&active, None));
    assert!(!remote_poll_should_refresh(&expired, Some(&viewer)));
}

#[test]
fn remote_status_targets_local_viewer_matches_direct_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/alice"],
        "cc": []
    });

    assert!(remote_status_targets_local_viewer(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_rejects_other_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/bob"],
        "cc": []
    });

    assert!(!remote_status_targets_local_viewer(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_account_rejects_followers_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/alice/followers"],
        "cc": []
    });

    assert!(!remote_status_targets_local_viewer_account(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_followers_matches_followers_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/alice/followers"],
        "cc": []
    });

    assert!(remote_status_targets_local_viewer_followers(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_followers_rejects_direct_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/alice"],
        "cc": []
    });

    assert!(!remote_status_targets_local_viewer_followers(
        &raw_status,
        &viewer,
        &config
    ));
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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
        super::ParsedStatusSearchQuery {
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

#[test]
fn validate_streaming_channel_request_accepts_known_streams() {
    assert_eq!(
        validate_streaming_channel_request(Some(" User "), None, None, None).unwrap(),
        "user"
    );
    assert_eq!(
        validate_streaming_channel_request(Some("hashtag:local"), Some("rust"), None, None)
            .unwrap(),
        "hashtag:local"
    );
    assert_eq!(
        validate_streaming_channel_request(Some("list"), None, Some("123"), None).unwrap(),
        "list"
    );
}

#[test]
fn validate_streaming_channel_request_rejects_unknown_or_incomplete_channels() {
    assert_eq!(
        validate_streaming_channel_request(Some("hashtag"), None, None, None).unwrap_err(),
        StreamingChannelValidationError::MissingTag
    );
    assert_eq!(
        validate_streaming_channel_request(Some("list"), None, None, None).unwrap_err(),
        StreamingChannelValidationError::MissingList
    );
    assert_eq!(
        validate_streaming_channel_request(Some("public"), None, None, Some("health")).unwrap_err(),
        StreamingChannelValidationError::UnknownChannelRequested
    );
}

#[test]
fn streaming_channel_requires_auth_matches_user_scoped_streams() {
    assert!(streaming_channel_requires_auth("user"));
    assert!(streaming_channel_requires_auth("user:notification"));
    assert!(streaming_channel_requires_auth("list"));
    assert!(streaming_channel_requires_auth("direct"));
    assert!(!streaming_channel_requires_auth("public"));
    assert!(!streaming_channel_requires_auth("hashtag"));
}

#[test]
fn validate_account_registration_request_requires_core_fields() {
    let details = validate_account_registration_request(&AccountRegistrationValidation {
        username: None,
        email: None,
        password_present: false,
        agreement: None,
    });
    assert_eq!(
        details.get("username"),
        Some(&vec!["can't be blank".to_owned()])
    );
    assert_eq!(
        details.get("email"),
        Some(&vec!["can't be blank".to_owned()])
    );
    assert_eq!(
        details.get("password"),
        Some(&vec!["can't be blank".to_owned()])
    );
    assert_eq!(
        details.get("agreement"),
        Some(&vec!["must be accepted".to_owned()])
    );
}

#[test]
fn validate_account_registration_request_rejects_invalid_username() {
    let details = validate_account_registration_request(&AccountRegistrationValidation {
        username: Some("alice-bob".to_owned()),
        email: Some("alice@example.com".to_owned()),
        password_present: true,
        agreement: Some(true),
    });
    assert_eq!(
        details.get("username"),
        Some(&vec![
            "must contain only letters, numbers and underscores".to_owned()
        ])
    );
}

#[test]
fn email_confirmation_message_uses_configured_instance_and_token() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let url = build_email_confirmation_url(&config, "tok en/1");

    assert_eq!(
        url,
        "https://social.example/auth/confirmation?confirmation_token=tok%20en%2F1"
    );
    assert_eq!(
        build_email_confirmation_subject(&config),
        "Confirm your cfwdon account"
    );
    assert!(build_email_confirmation_text(&config, &url).contains(&url));
    assert!(
        build_email_confirmation_html(&config, &url)
            .contains("https://social.example/auth/confirmation")
    );
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
fn extract_hashtags_from_text_deduplicates_and_normalizes() {
    assert_eq!(
        extract_hashtags_from_text("Hello #Rust #rust and #fediverse_test"),
        vec!["rust".to_owned(), "fediverse_test".to_owned()]
    );
}

#[test]
fn extract_hashtags_from_html_ignores_markup() {
    assert_eq!(
        extract_hashtags_from_html(
            "<p><a href=\"https://example/tags/rust\">#<span>Rust</span></a> and #Workers</p>"
        ),
        vec!["rust".to_owned(), "workers".to_owned()]
    );
}

#[test]
fn extract_mentions_from_text_finds_local_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_mentions_from_text(
        "@alice hi @bob@social.example and @carol@remote.example",
        &config,
    );
    assert_eq!(mentions.len(), 2);
    assert_eq!(mentions[0].username, "alice");
    assert_eq!(mentions[1].username, "bob");
}

#[test]
fn extract_mentions_from_text_deduplicates_local_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_mentions_from_text("@alice @alice@social.example", &config);
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].username, "alice");
}

#[test]
fn extract_account_handles_from_text_keeps_remote_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_account_handles_from_text("@alice @bob@remote.example @alice", &config);
    assert_eq!(mentions.len(), 2);
    assert_eq!(mentions[0].username, "alice");
    assert_eq!(mentions[0].domain.as_deref(), Some("social.example"));
    assert_eq!(mentions[1].username, "bob");
    assert_eq!(mentions[1].domain.as_deref(), Some("remote.example"));
}

#[test]
fn build_activitypub_delete_uses_status_audience_and_object_id() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
    let status = StatusRow {
        id: "status-1".to_owned(),
        account_id: account.id.clone(),
        ap_id: None,
        in_reply_to_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        _text_content: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: "public".to_owned(),
        sensitive: 0,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: "accepted".to_owned(),
        application_id: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    let activity = build_activitypub_delete_with_published_at(
        &config,
        &account,
        &status,
        "2026-01-02T00:00:00.000Z",
    )
    .unwrap();
    assert_eq!(activity.get("type"), Some(&serde_json::json!("Delete")));
    assert_eq!(
        activity.get("object"),
        Some(&serde_json::json!(
            "https://social.example/users/alice/statuses/status-1"
        ))
    );
    assert_eq!(
        activity.get("published"),
        Some(&serde_json::json!("2026-01-02T00:00:00.000Z"))
    );
    assert_eq!(
        activity.get("to"),
        Some(&serde_json::json!([
            "https://www.w3.org/ns/activitystreams#Public"
        ]))
    );
    assert_eq!(
        activity.pointer("/cc/0"),
        Some(&serde_json::json!(
            "https://social.example/users/alice/followers"
        ))
    );
}

#[test]
fn effective_local_quote_approval_defaults_to_public() {
    let status = StatusRow {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        _text_content: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: "public".to_owned(),
        sensitive: 0,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: "accepted".to_owned(),
        application_id: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    assert_eq!(effective_local_quote_approval_policy(&status), "public");
}

#[test]
fn effective_local_quote_approval_forces_private_status_to_nobody() {
    let status = StatusRow {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        _text_content: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: "private".to_owned(),
        sensitive: 0,
        language: Some("en".to_owned()),
        quote_approval_policy: Some("public".to_owned()),
        quote_state: "accepted".to_owned(),
        application_id: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    assert_eq!(effective_local_quote_approval_policy(&status), "nobody");
}

#[test]
fn initial_local_quote_approval_policy_forces_private_and_direct_to_nobody() {
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "followers".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
    let private_draft = cfwdon_domain::StatusDraft {
        text: "hello".to_owned(),
        visibility: cfwdon_domain::Visibility::FollowersOnly,
        spoiler_text: String::new(),
        sensitive: false,
        language: Some("en".to_owned()),
        quote_approval_policy: Some("public".to_owned()),
        in_reply_to_id: None,
        media_ids: Vec::new(),
        poll: None,
    };
    let direct_draft = cfwdon_domain::StatusDraft {
        visibility: cfwdon_domain::Visibility::Direct,
        ..private_draft.clone()
    };

    assert_eq!(
        initial_local_quote_approval_policy(&account, &private_draft),
        "nobody"
    );
    assert_eq!(
        initial_local_quote_approval_policy(&account, &direct_draft),
        "nobody"
    );
}

#[test]
fn initial_local_quote_approval_policy_uses_account_default_when_request_omits_it() {
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "followers".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
    let draft = cfwdon_domain::StatusDraft {
        text: "hello".to_owned(),
        visibility: cfwdon_domain::Visibility::Public,
        spoiler_text: String::new(),
        sensitive: false,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        in_reply_to_id: None,
        media_ids: Vec::new(),
        poll: None,
    };

    assert_eq!(
        initial_local_quote_approval_policy(&account, &draft),
        "followers"
    );
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
fn remote_quote_state_for_local_target_matches_policy_rules() {
    let mut status = StatusRow {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        _text_content: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: "public".to_owned(),
        sensitive: 0,
        language: Some("en".to_owned()),
        quote_approval_policy: Some("public".to_owned()),
        quote_state: "accepted".to_owned(),
        application_id: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    assert_eq!(
        remote_quote_state_for_local_target(&status, false, false),
        "accepted"
    );

    status.quote_approval_policy = Some("followers".to_owned());
    assert_eq!(
        remote_quote_state_for_local_target(&status, true, false),
        "accepted"
    );
    assert_eq!(
        remote_quote_state_for_local_target(&status, false, false),
        "pending"
    );

    status.quote_approval_policy = Some("nobody".to_owned());
    assert_eq!(
        remote_quote_state_for_local_target(&status, true, false),
        "pending"
    );

    status.visibility = "private".to_owned();
    status.quote_approval_policy = Some("public".to_owned());
    assert_eq!(
        remote_quote_state_for_local_target(&status, true, false),
        "pending"
    );
    assert_eq!(
        remote_quote_state_for_local_target(&status, true, true),
        "rejected"
    );
}

#[test]
fn effective_status_quote_state_defaults_to_accepted_without_quote() {
    let status = StatusRow {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        _text_content: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: "public".to_owned(),
        sensitive: 0,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: "revoked".to_owned(),
        application_id: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    assert_eq!(effective_status_quote_state(&status), "accepted");
    assert!(!status_has_active_quote(&status));
}

#[test]
fn status_has_active_quote_depends_on_quote_state() {
    let mut status = StatusRow {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        boost_of_uri: None,
        quote_of_uri: Some("https://remote.example/@bob/1".to_owned()),
        content_html: "<p>hello</p>".to_owned(),
        _text_content: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: "public".to_owned(),
        sensitive: 0,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: "pending".to_owned(),
        application_id: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    assert_eq!(effective_status_quote_state(&status), "pending");
    assert!(status_has_active_quote(&status));

    status.quote_state = "revoked".to_owned();
    assert_eq!(effective_status_quote_state(&status), "revoked");
    assert!(!status_has_active_quote(&status));
}

#[test]
fn remote_status_quote_helpers_follow_quote_state() {
    let mut status = RemoteStatusRow {
        id: "remote-1".to_owned(),
        actor_uri: "https://remote.example/users/bob".to_owned(),
        object_uri: "https://remote.example/users/bob/statuses/1".to_owned(),
        url: Some("https://remote.example/@bob/1".to_owned()),
        in_reply_to_uri: None,
        boost_of_uri: None,
        quote_of_uri: Some("https://social.example/users/alice/statuses/1".to_owned()),
        content_html: "<p>hello</p>".to_owned(),
        spoiler_text: String::new(),
        visibility: "public".to_owned(),
        sensitive: 0,
        language: Some("en".to_owned()),
        quote_state: "accepted".to_owned(),
        published_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    assert_eq!(effective_remote_status_quote_state(&status), "accepted");
    assert!(remote_status_has_active_quote(&status));

    status.quote_state = "revoked".to_owned();
    assert_eq!(effective_remote_status_quote_state(&status), "revoked");
    assert!(!remote_status_has_active_quote(&status));
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
fn build_status_update_activity_includes_quote_context_when_present() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
    let object = serde_json::json!({
        "id": "https://social.example/users/alice/statuses/status-1",
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": [],
        "_misskey_quote": "https://remote.example/statuses/quoted"
    });

    let activity = build_status_update_activity_with_id(
        &config,
        &account,
        object,
        "https://social.example/users/alice/statuses/status-1/updates/1",
        "2026-01-02T00:00:00.000Z",
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&activity).unwrap();

    assert!(json["@context"].is_array());
    assert!(
        json["@context"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| {
                entry
                    .get("_misskey_quote")
                    .and_then(|value| value.get("@id"))
                    .and_then(serde_json::Value::as_str)
                    == Some("https://misskey-hub.net/ns#_misskey_quote")
            }))
    );
}

#[test]
fn build_delete_quote_authorization_activity_uses_fep_044f_shape() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    let activity = serde_json::from_str::<serde_json::Value>(
        &build_delete_quote_authorization_activity(
            &config,
            &account,
            "https://remote.example/users/bob/statuses/1",
            "https://social.example/users/alice/statuses/2",
            "remote-status-1",
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(activity["type"], serde_json::json!("Delete"));
    assert_eq!(
        activity["actor"],
        serde_json::json!("https://social.example/users/alice")
    );
    assert_eq!(
        activity["object"]["type"],
        serde_json::json!("QuoteAuthorization")
    );
    assert_eq!(
        activity["object"]["attributedTo"],
        serde_json::json!("https://social.example/users/alice")
    );
    assert_eq!(
        activity["object"]["interactingObject"],
        serde_json::json!("https://remote.example/users/bob/statuses/1")
    );
    assert_eq!(
        activity["object"]["interactionTarget"],
        serde_json::json!("https://social.example/users/alice/statuses/2")
    );
    assert_eq!(
        activity["id"],
        serde_json::json!(
            "https://social.example/users/alice/statuses/2/quote_authorizations/remote-status-1#delete"
        )
    );
}

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
fn mastodon_report_response_serializes_forwarded_and_nullable_status_ids() {
    let target_account = MastodonAccountResponse {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        acct: "alice".to_owned(),
        uri: "https://social.example/users/alice".to_owned(),
        display_name: "Alice".to_owned(),
        locked: false,
        bot: false,
        group: false,
        discoverable: true,
        indexable: true,
        noindex: None,
        hide_collections: None,
        show_media: Some(true),
        show_media_replies: Some(true),
        show_featured: Some(true),
        last_status_at: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        note: String::new(),
        url: "https://social.example/@alice".to_owned(),
        avatar: String::new(),
        avatar_static: String::new(),
        header: String::new(),
        header_static: String::new(),
        emojis: Vec::new(),
        fields: Vec::new(),
        roles: Vec::new(),
        followers_count: 0,
        following_count: 0,
        statuses_count: 0,
        source: None,
    };
    let response = MastodonReportResponse {
        id: "report-1".to_owned(),
        action_taken: false,
        action_taken_at: None,
        category: "other".to_owned(),
        comment: "context".to_owned(),
        forwarded: false,
        created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        status_ids: None,
        target_account,
        rule_ids: None,
    };

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["forwarded"], serde_json::json!(false));
    assert!(value.get("forward").is_none());
    assert_eq!(value["status_ids"], serde_json::Value::Null);
    assert_eq!(value["rule_ids"], serde_json::Value::Null);
}

#[test]
fn extract_remote_profile_media_url_supports_string_object_and_array_shapes() {
    assert_eq!(
        extract_remote_profile_media_url(Some(&serde_json::json!(
            "https://cdn.example/avatar.png"
        ))),
        Some("https://cdn.example/avatar.png".to_owned())
    );
    assert_eq!(
        extract_remote_profile_media_url(Some(&serde_json::json!({
            "type": "Image",
            "url": {
                "type": "Link",
                "href": "https://cdn.example/header.webp"
            }
        }))),
        Some("https://cdn.example/header.webp".to_owned())
    );
    assert_eq!(
        extract_remote_profile_media_url(Some(&serde_json::json!([
            {"type": "Image", "url": "https://cdn.example/first.png"},
            {"type": "Image", "url": "https://cdn.example/second.png"}
        ]))),
        Some("https://cdn.example/first.png".to_owned())
    );
    assert_eq!(
        extract_remote_profile_media_url(Some(&serde_json::json!("javascript:alert(1)"))),
        None
    );
}

#[test]
fn remote_account_response_uses_cached_profile_media() {
    let actor = RemoteActorRow {
        actor_uri: "https://remote.example/users/alice".to_owned(),
        username: "alice".to_owned(),
        domain: "remote.example".to_owned(),
        locked: true,
        bot: true,
        discoverable: false,
        indexable: false,
        display_name: "Alice".to_owned(),
        summary_html: "<p>hello</p>".to_owned(),
        profile_url: Some("https://remote.example/@alice".to_owned()),
        avatar_url: Some("https://cdn.remote.example/avatar.png".to_owned()),
        header_url: Some("https://cdn.remote.example/header.png".to_owned()),
    };

    let response = MastodonAccountResponse::from_remote_actor(&actor);
    assert_eq!(response.avatar, "https://cdn.remote.example/avatar.png");
    assert_eq!(response.header, "https://cdn.remote.example/header.png");
    assert_eq!(response.url, "https://remote.example/@alice");
    assert!(response.locked);
    assert!(response.bot);
    assert!(!response.discoverable);
    assert!(!response.indexable);
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
fn activitypub_profile_attachments_use_property_value_shape() {
    let fields = vec![ProfileField {
        name: "Pronouns".to_owned(),
        value: "they/them".to_owned(),
    }];
    let rendered = activitypub_profile_attachments(&fields);
    assert_eq!(rendered[0]["type"], serde_json::json!("PropertyValue"));
    assert_eq!(rendered[0]["name"], serde_json::json!("Pronouns"));
    assert_eq!(rendered[0]["value"], serde_json::json!("they/them"));
}

#[test]
fn mastodon_account_response_reflects_locked_and_bot_flags() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let mut account = actor_fixture_account();
    account.locked = true;
    account.bot = true;

    let response = MastodonAccountResponse::from_account(&account, &config);
    assert!(response.locked);
    assert!(response.bot);
    assert_eq!(response.discoverable, account.discoverable);
}

#[test]
fn activitypub_actor_document_reflects_locked_and_bot_flags() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let mut account = actor_fixture_account();
    account.locked = true;
    account.bot = true;

    let actor = build_activitypub_actor_document(&config, &account);
    assert_eq!(actor.actor_type, "Service");
    assert!(actor.manually_approves_followers);
}

#[test]
fn build_update_person_activity_wraps_actor_document() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: "<p>hello</p>".to_owned(),
        bio_text: "hello".to_owned(),
        fields: vec![ProfileField {
            name: "Website".to_owned(),
            value: "https://example.com".to_owned(),
        }],
        locked: false,
        bot: false,
        discoverable: true,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    let activity = serde_json::from_str::<serde_json::Value>(
        &build_update_person_activity_with_id(
            &config,
            &account,
            "https://social.example/users/alice/updates/test-update",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(activity["type"], serde_json::json!("Update"));
    assert_eq!(
        activity["id"],
        serde_json::json!("https://social.example/users/alice/updates/test-update")
    );
    assert_eq!(
        activity["object"]["id"],
        serde_json::json!("https://social.example/users/alice")
    );
    assert_eq!(activity["object"]["discoverable"], serde_json::json!(true));
    assert_eq!(
        activity["object"]["attachment"][0]["name"],
        serde_json::json!("Website")
    );
}

#[test]
fn parse_remote_actor_profile_document_extracts_profile_fields() {
    let actor = serde_json::json!({
        "id": "https://remote.example/users/alice",
        "type": "Service",
        "preferredUsername": "Alice",
        "name": "Alice Example",
        "summary": "<p>remote bio</p>",
        "manuallyApprovesFollowers": true,
        "inbox": "https://remote.example/users/alice/inbox",
        "endpoints": {
            "sharedInbox": "https://remote.example/inbox"
        },
        "publicKey": {
            "id": "https://remote.example/users/alice#main-key",
            "publicKeyPem": "pem"
        },
        "url": "https://remote.example/@alice",
        "icon": {
            "type": "Image",
            "url": "https://cdn.remote.example/avatar.png"
        },
        "image": {
            "type": "Image",
            "url": "https://cdn.remote.example/header.png"
        }
    });

    let profile =
        parse_remote_actor_profile_document(&actor, "https://remote.example/users/fallback")
            .unwrap();
    assert_eq!(profile.actor_uri, "https://remote.example/users/alice");
    assert_eq!(profile.username, "alice");
    assert_eq!(profile.domain, "remote.example");
    assert_eq!(
        profile.inbox_uri,
        "https://remote.example/users/alice/inbox"
    );
    assert_eq!(
        profile.shared_inbox_uri.as_deref(),
        Some("https://remote.example/inbox")
    );
    assert_eq!(
        profile.public_key_id,
        "https://remote.example/users/alice#main-key"
    );
    assert_eq!(profile.display_name, "Alice Example");
    assert_eq!(profile.summary_html, "<p>remote bio</p>");
    assert_eq!(
        profile.profile_url.as_deref(),
        Some("https://remote.example/@alice")
    );
    assert_eq!(
        profile.avatar_url.as_deref(),
        Some("https://cdn.remote.example/avatar.png")
    );
    assert_eq!(
        profile.header_url.as_deref(),
        Some("https://cdn.remote.example/header.png")
    );
    assert!(profile.locked);
    assert!(profile.bot);
    assert!(profile.discoverable);
    assert!(profile.indexable);
}

#[test]
fn activitypub_actor_type_detection_matches_supported_profile_types() {
    assert!(is_activitypub_actor_type(Some("Person")));
    assert!(is_activitypub_actor_type(Some("Application")));
    assert!(is_activitypub_actor_type(Some("Group")));
    assert!(!is_activitypub_actor_type(Some("Note")));
    assert!(!is_activitypub_actor_type(None));
}

#[test]
fn normalize_status_poll_accepts_minimal_valid_poll() {
    let poll = normalize_status_poll(Some(CreateStatusPollRequest {
        options: Some(vec![" One ".to_owned(), "Two".to_owned(), String::new()]),
        expires_in: Some(600),
        multiple: Some(true),
        hide_totals: Some(true),
    }))
    .unwrap()
    .unwrap();

    assert_eq!(poll.options, vec!["One".to_owned(), "Two".to_owned()]);
    assert_eq!(poll.expires_in_seconds, 600);
    assert!(poll.multiple);
    assert!(poll.hide_totals);
}

#[test]
fn normalize_status_poll_rejects_invalid_shapes() {
    assert!(
        normalize_status_poll(Some(CreateStatusPollRequest {
            options: Some(vec!["Only one".to_owned()]),
            expires_in: Some(600),
            multiple: None,
            hide_totals: None,
        }))
        .is_err()
    );
    assert!(
        normalize_status_poll(Some(CreateStatusPollRequest {
            options: Some(vec!["One".to_owned(), "Two".to_owned()]),
            expires_in: Some(60),
            multiple: None,
            hide_totals: None,
        }))
        .is_err()
    );
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
fn build_remote_status_card_value_prefers_link_attachment_metadata() {
    let attachments = vec![crate::RemoteStatusAttachmentRow {
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
    let attachments = vec![crate::RemoteStatusAttachmentRow {
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

#[test]
fn is_admin_account_matches_configured_emails() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.admin_emails = vec!["admin@example.com".to_owned()];
    let mut account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "admin@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        locked: false,
        bot: false,
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_quote_policy: "public".to_owned(),
        default_sensitive: false,
        default_language: None,
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
    assert!(is_admin_account(&config, &account));

    account.access_email = "user@example.com".to_owned();
    assert!(!is_admin_account(&config, &account));
}

#[test]
fn directory_order_defaults_to_active_and_accepts_new() {
    assert_eq!(directory_order(None), super::DirectoryOrder::Active);
    assert_eq!(
        directory_order(Some("active")),
        super::DirectoryOrder::Active
    );
    assert_eq!(directory_order(Some("new")), super::DirectoryOrder::New);
    assert_eq!(directory_order(Some("NEW")), super::DirectoryOrder::New);
    assert_eq!(
        directory_order(Some("unexpected")),
        super::DirectoryOrder::Active
    );
}

#[test]
fn parse_csv_list_normalizes_and_deduplicates() {
    assert_eq!(
        parse_csv_list("Ja, en,ja ,, EN"),
        vec!["en".to_owned(), "ja".to_owned()]
    );
}

#[test]
fn notification_timestamp_sort_token_supports_sqlite_and_iso_shapes() {
    assert!(notification_timestamp_sort_token("2026-04-14 12:34:56").is_some());
    assert!(notification_timestamp_sort_token("2026-04-14T12:34:56.000Z").is_some());
    assert!(notification_timestamp_sort_token("not-a-date").is_none());
}

#[test]
fn notification_sort_key_orders_newer_timestamps_higher() {
    assert!(
        notification_sort_key("2026-04-14T12:34:56.000Z")
            > notification_sort_key("2026-04-14 12:33:56")
    );
}

#[test]
fn filter_notification_entries_by_query_applies_max_and_min_cursor() {
    let entries = vec![
        NotificationEntry {
            id: "notif-new".to_owned(),
            created_at: "2026-04-19T12:00:00.000Z".to_owned(),
            value: serde_json::json!({"id": "notif-new"}),
        },
        NotificationEntry {
            id: "notif-mid".to_owned(),
            created_at: "2026-04-19T11:00:00.000Z".to_owned(),
            value: serde_json::json!({"id": "notif-mid"}),
        },
        NotificationEntry {
            id: "notif-old".to_owned(),
            created_at: "2026-04-19T10:00:00.000Z".to_owned(),
            value: serde_json::json!({"id": "notif-old"}),
        },
    ];

    let older_than_mid = filter_notification_entries_by_query(
        entries.clone(),
        &NotificationsQuery {
            max_id: Some("notif-mid".to_owned()),
            ..NotificationsQuery::default()
        },
    );
    assert_eq!(
        older_than_mid
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec!["notif-old".to_owned()]
    );

    let newer_than_mid = filter_notification_entries_by_query(
        entries,
        &NotificationsQuery {
            min_id: Some("notif-mid".to_owned()),
            ..NotificationsQuery::default()
        },
    );
    assert_eq!(
        newer_than_mid
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec!["notif-new".to_owned()]
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

#[test]
fn context_async_refresh_id_uses_context_namespace() {
    assert_eq!(
        context_async_refresh_id("status-123"),
        "context:status-123:refresh"
    );
}

#[test]
fn format_async_refresh_header_value_includes_retry_and_result_count() {
    assert_eq!(
        format_async_refresh_header_value("context:status-123:refresh", 3, Some(0)),
        "id=\"context:status-123:refresh\", retry=3, result_count=0"
    );
}

#[test]
fn validate_poll_vote_submission_rejects_repeat_votes() {
    let error = validate_poll_vote_submission(1, true, 2).unwrap_err();
    assert_eq!(error, "you have already voted in this poll");
}

#[test]
fn validate_poll_vote_submission_rejects_multi_choice_for_single_choice_poll() {
    let error = validate_poll_vote_submission(0, false, 2).unwrap_err();
    assert_eq!(error, "poll does not allow multiple choices");
}

#[test]
fn instance_v2_document_uses_conservative_defaults() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.source_url = Some("https://codeberg.example/cfwdon".to_owned());
    config.instance_languages = vec!["ja".to_owned(), "en".to_owned()];
    config.contact_email = Some("admin@example.com".to_owned());
    config.instance_thumbnail_url = Some("https://media.example.com/site.png".to_owned());

    let document = build_instance_v2_document(
        &InstanceSummary {
            domain: "social.example".to_owned(),
            title: "cfwdon".to_owned(),
            description: "test instance".to_owned(),
            software: SoftwareInfo {
                name: "cfwdon".to_owned(),
                version: "0.1.0".to_owned(),
            },
            capabilities: InstanceCapabilities {
                federation: true,
                local_timeline: true,
                media_uploads: true,
            },
        },
        &config,
        3,
    );

    assert_eq!(
        document.get("domain"),
        Some(&serde_json::json!("social.example"))
    );
    assert_eq!(
        document.get("source_url"),
        Some(&serde_json::json!("https://codeberg.example/cfwdon"))
    );
    assert_eq!(
        document.pointer("/usage/users/active_month"),
        Some(&serde_json::json!(3))
    );
    assert_eq!(
        document.pointer("/api_versions/mastodon"),
        Some(&serde_json::json!(6))
    );
    assert_eq!(
        document.pointer("/configuration/urls/streaming"),
        Some(&serde_json::json!("wss://social.example"))
    );
    assert_eq!(
        document.pointer("/configuration/vapid/public_key"),
        Some(&serde_json::json!(""))
    );
    assert_eq!(
        document.pointer("/configuration/accounts/max_display_name_length"),
        Some(&serde_json::json!(30))
    );
    assert_eq!(
        document.pointer("/configuration/translation/enabled"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        document.pointer("/configuration/accounts/max_pinned_statuses"),
        Some(&serde_json::json!(5))
    );
    assert_eq!(
        document.pointer("/configuration/polls/max_options"),
        Some(&serde_json::json!(4))
    );
    assert_eq!(
        document.pointer("/configuration/media_attachments/image_matrix_limit"),
        Some(&serde_json::json!(16_777_216))
    );
    assert_eq!(
        document.pointer("/registrations/enabled"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        document.pointer("/contact/email"),
        Some(&serde_json::json!("admin@example.com"))
    );
    assert_eq!(
        document.pointer("/thumbnail/versions/@1x"),
        Some(&serde_json::json!("https://media.example.com/site.png"))
    );
    assert_eq!(
        document.pointer("/icon/0/src"),
        Some(&serde_json::json!("https://media.example.com/site.png"))
    );
}

#[test]
fn set_instance_translation_enabled_updates_instance_configuration() {
    let mut document = build_instance_v2_document(
        &InstanceSummary {
            domain: "social.example".to_owned(),
            title: "cfwdon".to_owned(),
            description: "test instance".to_owned(),
            software: SoftwareInfo {
                name: "cfwdon".to_owned(),
                version: "0.1.0".to_owned(),
            },
            capabilities: InstanceCapabilities {
                federation: true,
                local_timeline: true,
                media_uploads: true,
            },
        },
        &AppConfig::new("https://social.example", "cfwdon", "test instance"),
        3,
    );

    set_instance_translation_enabled(&mut document, true);

    assert_eq!(
        document.pointer("/configuration/translation/enabled"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn instance_v2_document_uses_configured_vapid_key() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.web_push_vapid_public_key = Some("BExamplePublicKey".to_owned());

    let document = build_instance_v2_document(
        &InstanceSummary {
            domain: "social.example".to_owned(),
            title: "cfwdon".to_owned(),
            description: "test instance".to_owned(),
            software: SoftwareInfo {
                name: "cfwdon".to_owned(),
                version: "0.1.0".to_owned(),
            },
            capabilities: InstanceCapabilities {
                federation: true,
                local_timeline: true,
                media_uploads: true,
            },
        },
        &config,
        1,
    );

    assert_eq!(
        document.pointer("/configuration/vapid/public_key"),
        Some(&serde_json::json!("BExamplePublicKey"))
    );
}

#[test]
fn instance_v2_document_advertises_configured_policy_urls() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.instance_extended_description_html = Some("<p>About</p>".to_owned());
    config.privacy_policy_html = Some("<p>Privacy</p>".to_owned());
    config.terms_of_service_html = Some("<p>Terms</p>".to_owned());

    let document = build_instance_v2_document(
        &InstanceSummary {
            domain: "social.example".to_owned(),
            title: "cfwdon".to_owned(),
            description: "test instance".to_owned(),
            software: SoftwareInfo {
                name: "cfwdon".to_owned(),
                version: "0.1.0".to_owned(),
            },
            capabilities: InstanceCapabilities {
                federation: true,
                local_timeline: true,
                media_uploads: true,
            },
        },
        &config,
        3,
    );

    assert_eq!(
        document.pointer("/configuration/urls/about"),
        Some(&serde_json::json!(
            "https://social.example/api/v1/instance/extended_description"
        ))
    );
    assert_eq!(
        document.pointer("/configuration/urls/privacy_policy"),
        Some(&serde_json::json!(
            "https://social.example/api/v1/instance/privacy_policy"
        ))
    );
    assert_eq!(
        document.pointer("/configuration/urls/terms_of_service"),
        Some(&serde_json::json!(
            "https://social.example/api/v1/instance/terms_of_service"
        ))
    );
}

#[test]
fn instance_v1_document_reports_mastodon_compatible_shape() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.contact_email = Some("admin@example.com".to_owned());
    config.instance_thumbnail_url = Some("https://media.example.com/site.png".to_owned());

    let document = build_instance_v1_document(
        &InstanceSummary {
            domain: "social.example".to_owned(),
            title: "cfwdon".to_owned(),
            description: "test instance".to_owned(),
            software: SoftwareInfo {
                name: "cfwdon".to_owned(),
                version: "0.1.0".to_owned(),
            },
            capabilities: InstanceCapabilities {
                federation: true,
                local_timeline: true,
                media_uploads: true,
            },
        },
        &config,
        2,
        5,
        9,
        4,
    );

    assert_eq!(
        document.get("uri"),
        Some(&serde_json::json!("social.example"))
    );
    assert_eq!(
        document.pointer("/stats/user_count"),
        Some(&serde_json::json!(5))
    );
    assert_eq!(
        document.pointer("/stats/status_count"),
        Some(&serde_json::json!(9))
    );
    assert_eq!(
        document.pointer("/stats/domain_count"),
        Some(&serde_json::json!(4))
    );
    assert_eq!(
        document.pointer("/contact_account"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(
        document.pointer("/urls/streaming_api"),
        Some(&serde_json::json!("wss://social.example"))
    );
    assert_eq!(
        document.pointer("/configuration/accounts/max_featured_tags"),
        Some(&serde_json::json!(10))
    );
    assert_eq!(
        document.pointer("/configuration/media_attachments/image_matrix_limit"),
        Some(&serde_json::json!(16_777_216))
    );
    assert_eq!(
        document.pointer("/configuration/polls/max_options"),
        Some(&serde_json::json!(4))
    );
}

#[test]
fn build_nodeinfo_documents_expose_expected_urls_and_counts() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let summary = InstanceSummary {
        domain: "social.example".to_owned(),
        title: "cfwdon".to_owned(),
        description: "test instance".to_owned(),
        software: SoftwareInfo {
            name: "cfwdon".to_owned(),
            version: "0.1.0".to_owned(),
        },
        capabilities: InstanceCapabilities {
            federation: true,
            local_timeline: true,
            media_uploads: true,
        },
    };

    let links = build_nodeinfo_links_document(&config);
    assert_eq!(
        links["links"][0]["href"],
        serde_json::json!(nodeinfo_url(&config))
    );

    let document = build_nodeinfo_document(&summary, &config, 5, 3, 8);
    assert_eq!(document["protocols"][0], serde_json::json!("activitypub"));
    assert_eq!(document["usage"]["users"]["total"], serde_json::json!(5));
    assert_eq!(
        document["usage"]["users"]["activeMonth"],
        serde_json::json!(3)
    );
    assert_eq!(document["usage"]["localPosts"], serde_json::json!(8));
}

#[test]
fn configured_html_document_builds_privacy_and_terms_shapes() {
    let privacy = configured_html_document(
        Some("<p>Privacy</p>"),
        Some("2026-01-01T00:00:00Z"),
        "1970-01-01T00:00:00Z",
        false,
    )
    .unwrap();
    assert_eq!(
        privacy,
        serde_json::json!({
            "updated_at": "2026-01-01T00:00:00Z",
            "content": "<p>Privacy</p>",
        })
    );

    let terms =
        configured_html_document(Some("<p>Terms</p>"), Some("2026-02-01"), "1970-01-01", true)
            .unwrap();
    assert_eq!(
        terms,
        serde_json::json!({
            "effective_date": "2026-02-01",
            "effective": true,
            "content": "<p>Terms</p>",
            "succeeded_by": serde_json::Value::Null,
        })
    );
}

#[test]
fn peer_authority_from_uri_normalizes_default_and_custom_ports() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    assert_eq!(
        peer_authority_from_uri(&config, "https://remote.example/users/alice"),
        Some("remote.example".to_owned())
    );
    assert_eq!(
        peer_authority_from_uri(&config, "https://remote.example:8443/users/alice"),
        Some("remote.example:8443".to_owned())
    );
    assert_eq!(
        peer_authority_from_uri(&config, "https://social.example/users/alice"),
        None
    );
}
