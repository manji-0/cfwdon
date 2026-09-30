use crate::oauth_apps::{
    OAuthAuthorizeRequest, auth0_login_url, auth0_logout_url,
    build_app_verify_credentials_document_from_parts, build_oauth_token_document,
    hash_account_password, oauth_authorize_url_from_form, parse_basic_authorization_header,
    redirect_uri_matches_registered, verify_account_password_hash,
};
use base64::Engine;
use cfwdon_core::AppConfig;
use std::collections::HashMap;
use url::Url;

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
fn account_password_hash_verifies_only_matching_password() {
    let hash = hash_account_password("correct horse battery staple", "salt-1");

    assert!(verify_account_password_hash(
        "correct horse battery staple",
        &hash
    ));
    assert!(!verify_account_password_hash("wrong password", &hash));
    assert!(!verify_account_password_hash(
        "correct horse battery staple",
        "pbkdf2-sha256$1$salt-1$invalid"
    ));
}

#[test]
fn auth0_login_url_uses_authorize_endpoint() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.auth0_domain = "tenant.auth0.com".to_owned();
    config.auth0_client_id = "client-id-1".to_owned();
    config.auth0_audience = "https://social.example/api".to_owned();
    let callback_url = Url::parse("https://social.example/oauth/auth0/callback").unwrap();

    let login_url = auth0_login_url(&config, &callback_url, "state-1", "challenge-1").unwrap();

    assert_eq!(login_url.scheme(), "https");
    assert_eq!(login_url.host_str(), Some("tenant.auth0.com"));
    assert_eq!(login_url.path(), "/authorize");
    let params = login_url.query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(
        params.get("client_id").map(|value| value.as_ref()),
        Some("client-id-1")
    );
    assert_eq!(
        params.get("audience").map(|value| value.as_ref()),
        Some("https://social.example/api")
    );
    assert_eq!(
        params.get("redirect_uri").map(|value| value.as_ref()),
        Some("https://social.example/oauth/auth0/callback")
    );
    assert_eq!(
        params.get("scope").map(|value| value.as_ref()),
        Some("openid profile email offline_access")
    );
    assert_eq!(
        params.get("state").map(|value| value.as_ref()),
        Some("state-1")
    );
    assert_eq!(
        params.get("code_challenge").map(|value| value.as_ref()),
        Some("challenge-1")
    );
    assert_eq!(
        params
            .get("code_challenge_method")
            .map(|value| value.as_ref()),
        Some("S256")
    );
}

#[test]
fn oauth_authorize_form_redirect_preserves_authorize_parameters_for_auth0() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.auth0_domain = "tenant.auth0.com".to_owned();
    config.auth0_client_id = "client-id-1".to_owned();
    config.auth0_audience = "https://social.example/api".to_owned();
    let base_url = Url::parse("https://social.example/oauth/authorize").unwrap();
    let authorize_url = oauth_authorize_url_from_form(
        &base_url,
        &OAuthAuthorizeRequest {
            response_type: Some("code".to_owned()),
            client_id: Some("client-1".to_owned()),
            redirect_uri: Some("mastodon://authorize".to_owned()),
            scope: Some("read write".to_owned()),
            state: Some("state-1".to_owned()),
            code_challenge: Some("challenge-1".to_owned()),
            code_challenge_method: Some("S256".to_owned()),
        },
    )
    .unwrap();

    let login_url = auth0_login_url(&config, &authorize_url, "state-1", "challenge-1").unwrap();

    let params = login_url.query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(
        params.get("redirect_uri").map(|value| value.as_ref()),
        Some(
            "https://social.example/oauth/authorize?response_type=code&client_id=client-1&redirect_uri=mastodon%3A%2F%2Fauthorize&scope=read+write&state=state-1&code_challenge=challenge-1&code_challenge_method=S256"
        )
    );
}

#[test]
fn mobile_custom_scheme_redirect_uri_allows_empty_path_slash_variants() {
    assert!(redirect_uri_matches_registered(
        "mastodon://authorize",
        "mastodon://authorize/"
    ));
    assert!(redirect_uri_matches_registered(
        "mastodon://authorize/",
        "mastodon://authorize"
    ));
    assert!(!redirect_uri_matches_registered(
        "https://app.example/callback",
        "https://app.example/callback/"
    ));
}

#[test]
fn auth0_logout_url_uses_v2_logout_with_return_to() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.auth0_domain = "tenant.auth0.com".to_owned();
    config.auth0_client_id = "client-id-1".to_owned();

    let logout_url = auth0_logout_url(&config).unwrap();

    assert_eq!(logout_url.scheme(), "https");
    assert_eq!(logout_url.host_str(), Some("tenant.auth0.com"));
    assert_eq!(logout_url.path(), "/v2/logout");
    let params = logout_url.query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(
        params.get("client_id").map(|value| value.as_ref()),
        Some("client-id-1")
    );
    assert_eq!(
        params.get("returnTo").map(|value| value.as_ref()),
        Some("https://social.example")
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
