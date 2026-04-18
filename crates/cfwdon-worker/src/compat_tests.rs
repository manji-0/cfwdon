use crate::account_store::AccountStats;
use crate::build_activitypub_actor_document;
use crate::build_featured_collection_document;
use crate::relationships::RelationshipResponse;
use crate::responses::{MastodonAccountResponse, MastodonStatusResponse};
use crate::status_store::StatusRow;
use crate::{
    build_default_privacy_policy_document, build_instance_activity_document,
    build_instance_v1_document, build_instance_v2_document, build_preferences_document,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    InstanceCapabilities, InstanceSummary, LocalAccount, ProfileField, SoftwareInfo,
};
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
        discoverable: true,
        default_post_visibility: "public".to_owned(),
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
        created_at: "2026-01-02T00:00:00.000Z".to_owned(),
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
        "/display_name",
        "/note",
        "/url",
        "/avatar",
        "/header",
        "/followers_count",
        "/following_count",
        "/statuses_count",
        "/fields/0/name",
        "/fields/0/value",
        "/source/privacy",
        "/source/sensitive",
        "/source/language",
        "/source/follow_requests_count",
        "/source/discoverable",
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
        "/display_name",
        "/note",
        "/url",
        "/avatar",
        "/header",
        "/fields/0/name",
        "/fields/0/value",
        "/followers_count",
        "/following_count",
        "/statuses_count",
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
        "/configuration/statuses/max_characters",
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
        "/configuration/urls/streaming",
        "/configuration/urls/about",
        "/configuration/urls/privacy_policy",
        "/configuration/urls/terms_of_service",
        "/configuration/statuses/max_characters",
        "/configuration/media_attachments/image_size_limit",
        "/configuration/polls/max_options",
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
