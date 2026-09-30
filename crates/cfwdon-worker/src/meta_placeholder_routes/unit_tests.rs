use crate::meta_placeholder_routes::{
    build_app_verify_credentials_document, build_donation_campaign_document,
    build_email_confirmation_html, build_email_confirmation_subject, build_email_confirmation_text,
    build_email_confirmation_url, build_oauth_authorization_server_document,
    build_oauth_userinfo_document,
};
use crate::test_fixtures::actor_fixture_account;
use cfwdon_core::AppConfig;

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
