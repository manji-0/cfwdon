use crate::identity::{
    instance_base_url, parse_csv_list, parse_lookup_handle, parse_webfinger_resource,
    peer_authority_from_uri, remote_account_rest_id, remote_actor_uri_from_rest_id,
};
use cfwdon_core::AppConfig;

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
fn parse_webfinger_resource_accepts_bare_handle_and_actor_urls() {
    let handle = parse_webfinger_resource("alice@example.com").unwrap();
    assert_eq!(handle.username, "alice");
    assert_eq!(handle.domain.as_deref(), Some("example.com"));

    let handle = parse_webfinger_resource("https://example.com/users/alice").unwrap();
    assert_eq!(handle.username, "alice");
    assert_eq!(handle.domain.as_deref(), Some("example.com"));

    let handle = parse_webfinger_resource("https://example.com/@Alice").unwrap();
    assert_eq!(handle.username, "alice");
    assert_eq!(handle.domain.as_deref(), Some("example.com"));
}

#[test]
fn parse_webfinger_resource_rejects_unsupported_forms() {
    let error = parse_webfinger_resource("ftp://example.com/users/alice").unwrap_err();
    assert!(
        error.to_string().contains("acct:user@domain") || error.to_string().contains("actor URL")
    );
    let error = parse_webfinger_resource("https://example.com/about").unwrap_err();
    assert!(error.to_string().contains("users"));
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
fn parse_csv_list_normalizes_and_deduplicates() {
    assert_eq!(
        parse_csv_list("Ja, en,ja ,, EN"),
        vec!["en".to_owned(), "ja".to_owned()]
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
