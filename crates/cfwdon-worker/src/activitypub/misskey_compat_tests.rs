use super::*;
use crate::activity_pub_status_input_from_object;
use crate::federation::parse_remote_actor_profile_document;

fn misskey_note_with_mfm_source() -> serde_json::Value {
    serde_json::json!({
        "id": "https://misskey.example/notes/note1",
        "type": "Note",
        "attributedTo": "https://misskey.example/users/alice",
        "content": "<p>hello <b>world</b></p>",
        "_misskey_content": "hello **world**",
        "source": {
            "content": "hello **world**",
            "mediaType": "text/x.misskeymarkdown"
        },
        "summary": "cw text",
        "sensitive": true,
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": ["https://misskey.example/users/alice/followers"],
        "published": "2026-07-29T00:00:00Z"
    })
}

fn misskey_note_quote_only() -> serde_json::Value {
    serde_json::json!({
        "id": "https://misskey.example/notes/quote1",
        "type": "Note",
        "attributedTo": "https://misskey.example/users/alice",
        "content": "<p>quoting</p>",
        "_misskey_quote": "https://social.example/users/bob/statuses/42",
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": []
    })
}

fn misskey_like_with_reaction() -> serde_json::Value {
    serde_json::json!({
        "id": "https://misskey.example/likes/1",
        "type": "Like",
        "actor": "https://misskey.example/users/alice",
        "object": "https://social.example/users/bob/statuses/42",
        "_misskey_reaction": ":blobcat:",
        "content": ":blobcat:",
        "to": ["https://social.example/users/bob"]
    })
}

fn misskey_emoji_react() -> serde_json::Value {
    serde_json::json!({
        "id": "https://misskey.example/reactions/1",
        "type": "EmojiReact",
        "actor": "https://misskey.example/users/alice",
        "object": "https://social.example/users/bob/statuses/42",
        "content": "⭐",
        "to": ["https://social.example/users/bob"]
    })
}

fn misskey_vote_activity() -> serde_json::Value {
    serde_json::json!({
        "id": "https://misskey.example/votes/1",
        "type": "Vote",
        "actor": "https://misskey.example/users/alice",
        "object": "https://social.example/users/bob/statuses/poll1",
        "name": "Option A"
    })
}

fn misskey_create_poll_vote_note() -> serde_json::Value {
    serde_json::json!({
        "id": "https://misskey.example/votes/create-1",
        "type": "Create",
        "actor": "https://misskey.example/users/alice",
        "object": {
            "id": "https://misskey.example/notes/vote-note-1",
            "type": "Note",
            "name": "Option A",
            "inReplyTo": "https://social.example/users/bob/statuses/poll1",
            "attributedTo": "https://misskey.example/users/alice",
            "to": ["https://social.example/users/bob"]
        }
    })
}

fn misskey_actor_endpoints_shared_inbox() -> serde_json::Value {
    serde_json::json!({
        "id": "https://misskey.example/users/alice",
        "type": "Person",
        "preferredUsername": "alice",
        "inbox": "https://misskey.example/users/alice/inbox",
        "endpoints": {
            "sharedInbox": "https://misskey.example/inbox"
        },
        "publicKey": {
            "id": "https://misskey.example/users/alice#main-key",
            "owner": "https://misskey.example/users/alice",
            "publicKeyPem": "pem"
        }
    })
}

fn misskey_actor_top_level_shared_inbox_only() -> serde_json::Value {
    serde_json::json!({
        "id": "https://misskey.example/users/alice",
        "type": "Person",
        "preferredUsername": "alice",
        "inbox": "https://misskey.example/users/alice/inbox",
        "sharedInbox": "https://misskey.example/inbox",
        "publicKey": {
            "id": "https://misskey.example/users/alice#main-key",
            "owner": "https://misskey.example/users/alice",
            "publicKeyPem": "pem"
        }
    })
}

#[test]
fn misskey_note_keeps_html_content_and_ignores_mfm_extensions() {
    let note = misskey_note_with_mfm_source();
    let input = activity_pub_status_input_from_object(&note);
    assert!(input.content_html.contains("hello"));
    assert!(!input.content_html.contains("**world**"));
    assert_eq!(input.spoiler_text.as_deref(), Some("cw text"));
    assert_eq!(input.sensitive, Some(true));
    assert!(note.get("_misskey_content").is_some());
    assert_eq!(
        note.pointer("/source/mediaType").and_then(|v| v.as_str()),
        Some("text/x.misskeymarkdown")
    );
}

#[test]
fn misskey_quote_only_note_resolves_quote_target() {
    let note = misskey_note_quote_only();
    assert_eq!(
        quote_target_uri_from_object(&note).as_deref(),
        Some("https://social.example/users/bob/statuses/42")
    );
    let input = activity_pub_status_input_from_object(&note);
    assert_eq!(
        input.misskey_quote.as_deref(),
        Some("https://social.example/users/bob/statuses/42")
    );
    assert!(input.quote_uri.is_none());
    assert!(input.quote_url.is_none());
}

#[test]
fn misskey_like_with_reaction_is_still_primary_like() {
    let activity = misskey_like_with_reaction();
    assert_eq!(activitypub_primary_type(&activity), Some("Like"));
    assert!(activitypub_has_type(&activity, "Like"));
    assert_eq!(
        activity
            .get("_misskey_reaction")
            .and_then(serde_json::Value::as_str),
        Some(":blobcat:")
    );
}

#[test]
fn misskey_emoji_react_and_vote_and_flag_are_distinct_unsupported_types() {
    assert_eq!(
        activitypub_primary_type(&misskey_emoji_react()),
        Some("EmojiReact")
    );
    assert_eq!(
        activitypub_primary_type(&misskey_vote_activity()),
        Some("Vote")
    );
    assert_eq!(
        activitypub_primary_type(&serde_json::json!({
            "type": "Flag",
            "object": "https://social.example/users/bob"
        })),
        Some("Flag")
    );
    assert!(!activitypub_has_type(&misskey_emoji_react(), "Like"));
}

#[test]
fn misskey_create_poll_vote_note_is_supported_remote_status_shape() {
    let activity = misskey_create_poll_vote_note();
    let object = activity.get("object").unwrap();
    assert!(object_has_supported_remote_status_type(object));
    assert_eq!(
        object.get("name").and_then(serde_json::Value::as_str),
        Some("Option A")
    );
    assert_eq!(
        object.get("inReplyTo").and_then(serde_json::Value::as_str),
        Some("https://social.example/users/bob/statuses/poll1")
    );
}

#[test]
fn misskey_actor_endpoints_shared_inbox_is_parsed() {
    let profile = parse_remote_actor_profile_document(
        &misskey_actor_endpoints_shared_inbox(),
        "https://misskey.example/users/alice",
    )
    .unwrap();
    assert_eq!(
        profile.shared_inbox_uri.as_deref(),
        Some("https://misskey.example/inbox")
    );
}

#[test]
fn misskey_actor_top_level_shared_inbox_alone_is_ignored() {
    let profile = parse_remote_actor_profile_document(
        &misskey_actor_top_level_shared_inbox_only(),
        "https://misskey.example/users/alice",
    )
    .unwrap();
    assert!(profile.shared_inbox_uri.is_none());
}

#[test]
fn misskey_non_person_actor_types_are_accepted() {
    for actor_type in ["Person", "Service", "Application", "Group", "Organization"] {
        assert!(
            is_activitypub_actor_type(Some(actor_type)),
            "expected {actor_type} to be accepted"
        );
    }
}

#[test]
fn misskey_emoji_react_does_not_resolve_like_inbox_target() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let like = misskey_like_with_reaction();
    let react = misskey_emoji_react();
    assert_eq!(
        extract_inbox_target_username(&config, &like).as_deref(),
        Some("bob")
    );
    assert!(extract_inbox_target_username(&config, &react).is_none());
}
