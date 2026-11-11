use crate::oauth_store::{
    oauth_access_token_auth_from_joined_row, oauth_access_token_has_any_scope_json,
    parse_bearer_authorization_header,
};
use cfwdon_domain::AccountApproval;

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

fn joined_token_row(pending_account_id: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "access_token": "sha256:abc",
        "oauth_app_id": 1,
        "scopes_json": "[\"read\"]",
        "id": "acct-1",
        "username": "alice",
        "access_email": "alice@example.com",
        "display_name": "alice",
        "bio_html": "",
        "bio_text": "",
        "fields_json": "[]",
        "locked": 0,
        "bot": 0,
        "discoverable": 0,
        "default_post_visibility": "public",
        "default_quote_policy": "public",
        "default_sensitive": 0,
        "default_language": null,
        "avatar_object_key": null,
        "avatar_content_type": null,
        "header_object_key": null,
        "header_content_type": null,
        "private_key_jwk": "",
        "public_key_pem": "pem",
        "created_at": "2026-01-01T00:00:00Z",
        "pending_account_id": pending_account_id,
    })
}

#[test]
fn a_tokens_account_carries_its_approval() {
    for (pending_account_id, expected) in [
        (None, AccountApproval::Approved),
        (Some("acct-1"), AccountApproval::Pending),
    ] {
        let owner = oauth_access_token_auth_from_joined_row(joined_token_row(pending_account_id))
            .unwrap()
            .unwrap()
            .account
            .unwrap();
        assert_eq!(owner.account.id(), "acct-1");
        assert_eq!(owner.approval, expected);
    }
}
