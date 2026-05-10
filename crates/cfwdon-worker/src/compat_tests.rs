use crate::account_store::AccountStats;
use crate::build_activitypub_actor_document;
use crate::build_announcements_document;
use crate::build_app_verify_credentials_document;
use crate::build_featured_collection_document;
use crate::relationships::RelationshipResponse;
use crate::responses::{MastodonAccountResponse, MastodonStatusResponse};
use crate::status_store::StatusRow;
use crate::{
    build_default_privacy_policy_document, build_donation_campaign_document,
    build_instance_activity_document, build_instance_v1_document, build_instance_v2_document,
    build_oauth_authorization_server_document, build_oauth_userinfo_document,
    build_preferences_document, build_translation_document, scheduled_status_document,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    InstanceCapabilities, InstanceSummary, LocalAccount, ProfileField, SoftwareInfo,
};
use std::collections::{HashMap, HashSet};
use time::{Date, Month, PrimitiveDateTime, Time, UtcOffset};

fn fixture_account() -> LocalAccount {
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

fn fixture_config() -> AppConfig {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.media_public_base_url = Some("https://media.example.com".to_owned());
    config.source_url = Some("https://codeberg.example/cfwdon".to_owned());
    config.instance_thumbnail_url = Some("https://media.example.com/site.png".to_owned());
    config.contact_email = Some("admin@example.com".to_owned());
    config.instance_extended_description_html = Some("<p>About cfwdon</p>".to_owned());
    config.privacy_policy_html = Some("<p>Privacy policy</p>".to_owned());
    config.terms_of_service_html = Some("<p>Terms</p>".to_owned());
    config
}

fn fixture_stats() -> AccountStats {
    AccountStats {
        followers_count: 3,
        following_count: 5,
        statuses_count: 8,
        last_status_at: Some("2026-01-02".to_owned()),
    }
}

fn fixture_status() -> StatusRow {
    StatusRow {
        id: "status-1".to_owned(),
        account_id: "acct-1".to_owned(),
        ap_id: Some("https://social.example/users/alice/statuses/status-1".to_owned()),
        in_reply_to_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>Hello <span class=\"h-card\"><a href=\"https://social.example/@bob\" class=\"u-url mention\">@<span>bob</span></a></span> #Workers</p>".to_owned(),
        _text_content: "Hello @bob #Workers".to_owned(),
        spoiler_text: String::new(),
        visibility: "public".to_owned(),
        sensitive: 0,
        language: Some("ja".to_owned()),
        quote_approval_policy: None,
        quote_state: "accepted".to_owned(),
        application_id: None,
        created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        updated_at: None,
    }
}

fn assert_has_pointer(value: &serde_json::Value, pointer: &str) {
    assert!(
        value.pointer(pointer).is_some(),
        "expected JSON pointer {pointer} to exist in {value}"
    );
}

#[test]
fn compatibility_verify_credentials_shape_is_stable() {
    let value = serde_json::to_value(MastodonAccountResponse::from_credentials_account(
        &fixture_account(),
        &fixture_config(),
        &fixture_stats(),
    ))
    .unwrap();

    for pointer in [
        "/id",
        "/username",
        "/acct",
        "/uri",
        "/display_name",
        "/group",
        "/discoverable",
        "/indexable",
        "/note",
        "/url",
        "/avatar",
        "/header",
        "/emojis",
        "/followers_count",
        "/following_count",
        "/statuses_count",
        "/show_media",
        "/show_media_replies",
        "/show_featured",
        "/roles",
        "/last_status_at",
        "/last_status_at",
        "/fields/0/name",
        "/fields/0/value",
        "/source/privacy",
        "/source/sensitive",
        "/source/language",
        "/source/attribution_domains",
        "/source/follow_requests_count",
        "/source/discoverable",
        "/source/indexable",
        "/source/quote_policy",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_public_account_shape_is_stable() {
    let value = serde_json::to_value(MastodonAccountResponse::from_account_with_stats(
        &fixture_account(),
        &fixture_config(),
        &fixture_stats(),
    ))
    .unwrap();

    for pointer in [
        "/id",
        "/username",
        "/acct",
        "/uri",
        "/display_name",
        "/group",
        "/discoverable",
        "/indexable",
        "/note",
        "/url",
        "/avatar",
        "/header",
        "/fields/0/name",
        "/fields/0/value",
        "/followers_count",
        "/following_count",
        "/statuses_count",
        "/show_media",
        "/show_media_replies",
        "/show_featured",
        "/roles",
        "/last_status_at",
        "/last_status_at",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_activitypub_actor_shape_is_stable() {
    let value = serde_json::to_value(build_activitypub_actor_document(
        &fixture_config(),
        &fixture_account(),
    ))
    .unwrap();

    for pointer in [
        "/@context/0",
        "/id",
        "/type",
        "/preferredUsername",
        "/name",
        "/summary",
        "/inbox",
        "/outbox",
        "/followers",
        "/following",
        "/featured",
        "/featuredTags",
        "/endpoints/sharedInbox",
        "/attachment/0/name",
        "/attachment/0/value",
        "/publicKey/id",
        "/publicKey/owner",
        "/publicKey/publicKeyPem",
        "/discoverable",
        "/published",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_status_shape_is_stable() {
    let value = serde_json::to_value(MastodonStatusResponse::from_row(
        &fixture_status(),
        &fixture_account(),
        &fixture_config(),
        None,
        Vec::new(),
    ))
    .unwrap();

    for pointer in [
        "/id",
        "/created_at",
        "/visibility",
        "/uri",
        "/url",
        "/content",
        "/muted",
        "/pinned",
        "/account/id",
        "/account/acct",
        "/media_attachments",
        "/mentions",
        "/tags/0/name",
        "/emojis",
        "/quote_approval",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_relationship_shape_is_stable() {
    let value = serde_json::to_value(RelationshipResponse {
        id: "acct-2".to_owned(),
        following: true,
        showing_reblogs: true,
        notifying: false,
        languages: Some(vec!["ja".to_owned()]),
        followed_by: false,
        blocking: false,
        blocked_by: false,
        muting: true,
        muting_notifications: true,
        muting_expires_at: None,
        requested: false,
        requested_by: false,
        domain_blocking: false,
        endorsed: false,
        note: String::new(),
    })
    .unwrap();

    for pointer in [
        "/id",
        "/following",
        "/showing_reblogs",
        "/notifying",
        "/languages/0",
        "/followed_by",
        "/blocking",
        "/muting",
        "/requested",
        "/domain_blocking",
        "/note",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_preferences_shape_is_stable() {
    let value = build_preferences_document(&fixture_account());

    for pointer in [
        "/posting:default:visibility",
        "/posting:default:sensitive",
        "/posting:default:language",
        "/posting:default:quote_policy",
        "/posting:default:privacy",
        "/posting:default:media_sensitive",
        "/posting:default:content_type",
        "/notifications:follow",
        "/notifications:mention",
        "/web:theme",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_quote_policy_reflects_account_default() {
    let mut account = fixture_account();
    account.default_quote_policy = "followers".to_owned();

    let preferences = build_preferences_document(&account);
    assert_eq!(
        preferences.pointer("/posting:default:quote_policy"),
        Some(&serde_json::json!("followers"))
    );

    let credentials = serde_json::to_value(MastodonAccountResponse::from_credentials_account(
        &account,
        &fixture_config(),
        &fixture_stats(),
    ))
    .unwrap();
    assert_eq!(
        credentials.pointer("/source/quote_policy"),
        Some(&serde_json::json!("followers"))
    );
}

#[test]
fn compatibility_instance_activity_shape_is_stable() {
    let week_floor = PrimitiveDateTime::new(
        Date::from_calendar_date(2026, Month::March, 30).unwrap(),
        Time::MIDNIGHT,
    )
    .assume_offset(UtcOffset::UTC);
    let value = build_instance_activity_document(week_floor, &[(1, 0, 1); 12]);

    let items = value
        .as_array()
        .expect("instance activity should be an array");
    assert_eq!(items.len(), 12);
    for item in items {
        assert_has_pointer(item, "/week");
        assert_has_pointer(item, "/statuses");
        assert_has_pointer(item, "/logins");
        assert_has_pointer(item, "/registrations");
    }
}

#[test]
fn compatibility_privacy_policy_fallback_shape_is_stable() {
    let value = build_default_privacy_policy_document("test instance");
    assert_has_pointer(&value, "/updated_at");
    assert_has_pointer(&value, "/content");
}

#[test]
fn compatibility_oauth_authorization_server_shape_is_stable() {
    let value = build_oauth_authorization_server_document(&fixture_config());

    for pointer in [
        "/issuer",
        "/authorization_endpoint",
        "/token_endpoint",
        "/userinfo_endpoint",
        "/revocation_endpoint",
        "/app_registration_endpoint",
        "/response_types_supported/0",
        "/response_modes_supported/0",
        "/grant_types_supported/0",
        "/scopes_supported/0",
        "/token_endpoint_auth_methods_supported/0",
        "/code_challenge_methods_supported/0",
        "/service_documentation",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_oauth_userinfo_shape_is_stable() {
    let value = build_oauth_userinfo_document(&fixture_config(), &fixture_account());

    for pointer in [
        "/iss",
        "/sub",
        "/preferred_username",
        "/name",
        "/profile",
        "/picture",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_app_verify_credentials_shape_is_stable() {
    let value = build_app_verify_credentials_document(&fixture_config());

    for pointer in [
        "/id",
        "/name",
        "/website",
        "/scopes/0",
        "/redirect_uris/0",
        "/redirect_uri",
        "/vapid_key",
    ] {
        assert_has_pointer(&value, pointer);
    }

    assert_eq!(value.pointer("/client_id"), None);
    assert_eq!(value.pointer("/client_secret"), None);
}

#[test]
fn compatibility_scheduled_status_shape_is_stable() {
    let value = scheduled_status_document("sched-1");

    for pointer in [
        "/id",
        "/scheduled_at",
        "/params/poll",
        "/params/text",
        "/params/language",
        "/params/media_ids",
        "/params/sensitive",
        "/params/visibility",
        "/params/idempotency",
        "/params/scheduled_at",
        "/params/spoiler_text",
        "/params/application_id",
        "/params/in_reply_to_id",
        "/params/with_rate_limit",
        "/media_attachments",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_translation_shape_is_stable() {
    let value = build_translation_document(&serde_json::json!({
        "content": "<p>Hello world</p>",
        "spoiler_text": "cw",
        "language": "ja",
        "media_attachments": [
            {
                "id": "media-1",
                "description": "alt text"
            }
        ],
        "poll": {
            "id": "poll-1",
            "options": [
                { "title": "One" }
            ]
        }
    }));

    for pointer in [
        "/content",
        "/spoiler_text",
        "/language",
        "/poll/id",
        "/poll/options/0/title",
        "/media_attachments/0/id",
        "/media_attachments/0/description",
        "/detected_source_language",
        "/provider",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_search_shape_includes_collections() {
    let value = serde_json::to_value(crate::MastodonSearchResponse::default()).unwrap();
    assert_has_pointer(&value, "/collections");
}

#[test]
fn compatibility_donation_campaign_shape_is_stable() {
    let mut config = fixture_config();
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
    let value = build_donation_campaign_document(&config).unwrap();

    for pointer in [
        "/id",
        "/banner_message",
        "/banner_button_text",
        "/donation_message",
        "/donation_button_text",
        "/donation_success_post",
        "/amounts/one_time/EUR/0",
        "/amounts/monthly/USD/0",
        "/default_currency",
        "/donation_url",
        "/locale",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_announcements_shape_is_stable() {
    let mut config = fixture_config();
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
        (2, true),
    )]);
    let value = serde_json::Value::Array(build_announcements_document(
        &config,
        &read_ids,
        &reaction_state,
    ));

    for pointer in [
        "/0/id",
        "/0/content",
        "/0/starts_at",
        "/0/ends_at",
        "/0/all_day",
        "/0/published_at",
        "/0/updated_at",
        "/0/read",
        "/0/mentions",
        "/0/statuses",
        "/0/tags",
        "/0/emojis",
        "/0/reactions/0/name",
        "/0/reactions/0/count",
        "/0/reactions/0/me",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_instance_v1_shape_is_stable() {
    let value = build_instance_v1_document(
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
        &fixture_config(),
        3,
        5,
        8,
        2,
    );

    for pointer in [
        "/uri",
        "/title",
        "/short_description",
        "/description",
        "/version",
        "/stats/user_count",
        "/stats/status_count",
        "/stats/domain_count",
        "/configuration/accounts/max_featured_tags",
        "/configuration/statuses/max_characters",
        "/configuration/media_attachments/image_matrix_limit",
        "/configuration/polls/max_options",
        "/urls/streaming_api",
        "/contact_account",
        "/rules",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_instance_v2_shape_is_stable() {
    let value = build_instance_v2_document(
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
        &fixture_config(),
        3,
    );

    for pointer in [
        "/domain",
        "/title",
        "/version",
        "/source_url",
        "/usage/users/active_month",
        "/thumbnail/url",
        "/thumbnail/versions/@1x",
        "/icon/0/src",
        "/configuration/urls/streaming",
        "/configuration/urls/about",
        "/configuration/urls/privacy_policy",
        "/configuration/urls/terms_of_service",
        "/configuration/vapid/public_key",
        "/configuration/accounts/max_display_name_length",
        "/configuration/accounts/max_pinned_statuses",
        "/configuration/statuses/max_characters",
        "/configuration/media_attachments/image_size_limit",
        "/configuration/media_attachments/image_matrix_limit",
        "/configuration/polls/max_options",
        "/configuration/timelines_access/trending_link_feeds/local",
        "/api_versions/mastodon",
        "/registrations/enabled",
        "/contact/email",
        "/rules",
    ] {
        assert_has_pointer(&value, pointer);
    }
}

#[test]
fn compatibility_featured_collection_shape_is_stable() {
    let value = build_featured_collection_document(
        &fixture_config(),
        "alice",
        &[
            "https://social.example/users/alice/statuses/1".to_owned(),
            "https://social.example/users/alice/statuses/2".to_owned(),
        ],
    );

    for pointer in [
        "/@context",
        "/id",
        "/type",
        "/totalItems",
        "/orderedItems/0/id",
        "/orderedItems/1/id",
    ] {
        assert_has_pointer(&value, pointer);
    }
}
