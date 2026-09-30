use crate::federation::{
    RemoteActorProfile, extract_remote_profile_media_url, parse_http_url_parts,
    parse_remote_actor_profile_document,
};
use crate::responses::MastodonAccountResponse;

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
fn remote_account_response_uses_fetched_profile_media() {
    let actor = RemoteActorProfile {
        actor_uri: "https://remote.example/users/carol".to_owned(),
        username: "carol".to_owned(),
        domain: "remote.example".to_owned(),
        locked: true,
        bot: false,
        discoverable: true,
        indexable: false,
        inbox_uri: "https://remote.example/users/carol/inbox".to_owned(),
        shared_inbox_uri: Some("https://remote.example/inbox".to_owned()),
        public_key_id: "https://remote.example/users/carol#main-key".to_owned(),
        public_key_pem: "-----BEGIN PUBLIC KEY-----\nMIIB\n-----END PUBLIC KEY-----".to_owned(),
        display_name: "Carol Remote".to_owned(),
        summary_html: "<p>fresh profile</p>".to_owned(),
        profile_url: Some("https://remote.example/@carol".to_owned()),
        avatar_url: Some("https://cdn.remote.example/carol-avatar.png".to_owned()),
        header_url: Some("https://cdn.remote.example/carol-header.png".to_owned()),
    };

    let response = MastodonAccountResponse::from_remote_actor_profile(&actor);
    assert_eq!(response.username, "carol");
    assert_eq!(response.acct, "carol@remote.example");
    assert_eq!(response.display_name, "Carol Remote");
    assert_eq!(response.note, "<p>fresh profile</p>");
    assert_eq!(response.url, "https://remote.example/@carol");
    assert_eq!(
        response.avatar,
        "https://cdn.remote.example/carol-avatar.png"
    );
    assert_eq!(
        response.header,
        "https://cdn.remote.example/carol-header.png"
    );
    assert!(response.locked);
    assert!(!response.indexable);
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
            "owner": "https://remote.example/users/alice",
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
fn parse_remote_actor_profile_document_rejects_cross_authority_id() {
    let actor = serde_json::json!({
        "id": "https://victim.social/users/alice",
        "type": "Person",
        "inbox": "https://evil.example/users/alice/inbox",
        "publicKey": {
            "id": "https://evil.example/users/alice#main-key",
            "publicKeyPem": "pem"
        }
    });

    let error =
        parse_remote_actor_profile_document(&actor, "https://evil.example/fake").unwrap_err();
    assert!(error.to_string().contains("authority"));
}

#[test]
fn parse_remote_actor_profile_document_rejects_cross_authority_inbox() {
    let actor = serde_json::json!({
        "id": "https://remote.example/users/alice",
        "type": "Person",
        "inbox": "https://evil.example/inbox",
        "publicKey": {
            "id": "https://remote.example/users/alice#main-key",
            "publicKeyPem": "pem"
        }
    });

    let error = parse_remote_actor_profile_document(&actor, "https://remote.example/users/alice")
        .unwrap_err();
    assert!(error.to_string().contains("inbox"));
}

#[test]
fn parse_remote_actor_profile_document_uses_fallback_identity_fields() {
    let actor = serde_json::json!({
        "type": "Person",
        "inbox": "https://remote.example/users/bob/inbox",
        "publicKey": {
            "id": "https://remote.example/users/bob#main-key",
            "publicKeyPem": "pem"
        }
    });

    let profile =
        parse_remote_actor_profile_document(&actor, "https://remote.example/users/Bob").unwrap();
    assert_eq!(profile.actor_uri, "https://remote.example/users/Bob");
    assert_eq!(profile.username, "bob");
    assert_eq!(profile.domain, "remote.example");
    assert_eq!(profile.display_name, "");
    assert_eq!(profile.summary_html, "");
    assert!(!profile.locked);
    assert!(!profile.bot);
    assert!(profile.discoverable);
    assert!(profile.indexable);
}
