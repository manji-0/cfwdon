use crate::oauth_store::{
    oauth_access_token_has_any_scope_json, parse_bearer_authorization_header,
};

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
