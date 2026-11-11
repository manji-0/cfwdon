use crate::federation::CachedDomainBlock;
use crate::identity::nodeinfo_url;
use crate::instance::{
    build_announcements_document, build_instance_v1_document, build_instance_v2_document,
    build_nodeinfo_document_with_halfyear, build_nodeinfo_links_document,
    instance_open_registrations, public_domain_blocks, set_instance_translation_enabled,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    DomainBlockPolicy, DomainBlockSeverity, InstanceCapabilities, InstanceSummary, SoftwareInfo,
};
use std::collections::{HashMap, HashSet};

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
        Some(&serde_json::json!(8))
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
        Some(&serde_json::json!(instance_open_registrations()))
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
fn instance_v2_document_advertises_policy_urls_without_configured_html() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
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

    let document = build_nodeinfo_document_with_halfyear(&summary, &config, 5, 3, 4, 8);
    assert_eq!(document["protocols"][0], serde_json::json!("activitypub"));
    assert_eq!(document["usage"]["users"]["total"], serde_json::json!(5));
    assert_eq!(
        document["usage"]["users"]["activeMonth"],
        serde_json::json!(3)
    );
    assert_eq!(
        document["usage"]["users"]["activeHalfyear"],
        serde_json::json!(4)
    );
    assert_eq!(document["usage"]["localPosts"], serde_json::json!(8));
}

#[test]
fn public_domain_blocks_use_mastodon_entity_shape() {
    let block = |domain: &str, severity, obfuscate, comment: Option<&str>| CachedDomainBlock {
        domain: domain.to_owned(),
        policy: DomainBlockPolicy {
            severity,
            reject_media: false,
            reject_reports: false,
        },
        obfuscate,
        public_comment: comment.map(str::to_owned),
    };
    let blocks = [
        block("blocked.example", DomainBlockSeverity::Suspend, false, None),
        block(
            "example.com",
            DomainBlockSeverity::Silence,
            true,
            Some("spam"),
        ),
        block("watched.example", DomainBlockSeverity::Noop, false, None),
    ];
    let document = serde_json::to_value(public_domain_blocks(&blocks)).unwrap();
    assert_eq!(
        document,
        serde_json::json!([
            {
                "domain": "blocked.example",
                "digest": "1f30273176bf43428242811a3bd5e04653804cff849e2ac73a14aa6e00c66a48",
                "severity": "suspend",
                "comment": null,
            },
            {
                "domain": "exa****.*om",
                "digest": "a379a6f6eeafb9a55e378c118034e2751e682fab9f2d30ab13d2125586ce1947",
                "severity": "silence",
                "comment": "spam",
            },
        ])
    );
}
