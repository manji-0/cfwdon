use crate::activitypub::{
    activitypub_audiences_for_visibility, activitypub_media_attachment_type,
    build_accept_quote_request_activity_with_id, build_activitypub_actor_document,
    build_add_featured_activity_with_id, build_delete_quote_authorization_activity,
    build_quote_authorization_object, build_quote_request_object,
    build_reject_quote_request_activity_with_id, build_remove_featured_activity_with_id,
    build_status_update_activity_with_id, build_update_person_activity_with_id,
    extract_inbox_target_username, extract_remote_note_object, follow_targets_local_actor,
    is_activitypub_actor_type, is_follow_undo, local_status_ap_id, local_username_from_actor_uri,
    local_username_from_status_uri, note_targets_account_or_followers, note_targets_followers,
    note_targets_public, object_attributed_to_remote_actor, quote_authorization_uri,
    quote_target_uri_from_object, visibility_from_activitypub_object,
};
use crate::test_fixtures::{actor_fixture_account, actor_fixture_account_locked_bot};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalStatus;

#[test]
fn extract_remote_note_object_supports_note_question_and_create_wrappers() {
    let note = serde_json::json!({"type":"Note","id":"https://remote.example/notes/1"});
    assert_eq!(
        extract_remote_note_object(&note)
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("https://remote.example/notes/1")
    );

    let question = serde_json::json!({
        "type":"Question",
        "id":"https://remote.example/notes/3",
        "oneOf":[
            {"type":"Note","name":"yes","replies":{"totalItems":2}},
            {"type":"Note","name":"no","replies":{"totalItems":1}}
        ]
    });
    assert_eq!(
        extract_remote_note_object(&question)
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("https://remote.example/notes/3")
    );

    let create = serde_json::json!({
        "type":"Create",
        "object":{"type":"Question","id":"https://remote.example/notes/2"}
    });
    assert_eq!(
        extract_remote_note_object(&create)
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str),
        Some("https://remote.example/notes/2")
    );
}

#[test]
fn extract_remote_note_object_rejects_non_note_documents() {
    let actor = serde_json::json!({"type":"Person","id":"https://remote.example/users/alice"});
    assert!(extract_remote_note_object(&actor).is_none());
}

#[test]
fn object_attributed_to_remote_actor_accepts_matching_actor_and_fallback() {
    let activity = serde_json::json!({
        "actor": "https://remote.example/users/alice",
        "object": {
            "type": "Note",
            "id": "https://remote.example/users/alice/statuses/1",
            "attributedTo": "https://remote.example/users/alice"
        }
    });
    assert!(object_attributed_to_remote_actor(
        activity.get("object").unwrap(),
        &activity,
        "https://remote.example/users/alice",
    ));

    let fallback_activity = serde_json::json!({
        "actor": "https://remote.example/users/alice",
        "object": {
            "type": "Note",
            "id": "https://remote.example/users/alice/statuses/2"
        }
    });
    assert!(object_attributed_to_remote_actor(
        fallback_activity.get("object").unwrap(),
        &fallback_activity,
        "https://remote.example/users/alice",
    ));
}

#[test]
fn object_attributed_to_remote_actor_rejects_mismatch() {
    let activity = serde_json::json!({
        "actor": "https://remote.example/users/alice",
        "object": {
            "type": "Note",
            "id": "https://remote.example/users/alice/statuses/3",
            "attributedTo": "https://evil.example/users/mallory"
        }
    });
    assert!(!object_attributed_to_remote_actor(
        activity.get("object").unwrap(),
        &activity,
        "https://remote.example/users/alice",
    ));
}

#[test]
fn build_status_update_activity_wraps_question_object() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = actor_fixture_account();
    let object = serde_json::json!({
        "id": "https://social.example/users/alice/statuses/status-1",
        "type": "Question",
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": ["https://social.example/users/alice/followers"],
    });

    let payload = build_status_update_activity_with_id(
        &config,
        &account,
        object,
        "https://social.example/users/alice/statuses/status-1/updates/test",
        "2026-02-01T00:00:00Z",
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&payload).unwrap();
    assert_eq!(value["type"], serde_json::json!("Update"));
    assert_eq!(
        value["id"],
        serde_json::json!("https://social.example/users/alice/statuses/status-1/updates/test")
    );
    assert_eq!(value["object"]["type"], serde_json::json!("Question"));
    assert_eq!(
        value["to"],
        serde_json::json!(["https://www.w3.org/ns/activitystreams#Public"])
    );
}

#[test]
fn build_featured_collection_activities_target_followers_collection() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = actor_fixture_account();

    let add = serde_json::from_str::<serde_json::Value>(
        &build_add_featured_activity_with_id(
            &config,
            &account,
            "https://social.example/users/alice/statuses/123",
            "https://social.example/users/alice/collections/featured/add/test",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(add["type"], "Add");
    assert_eq!(
        add["target"],
        "https://social.example/users/alice/collections/featured"
    );
    assert_eq!(
        add["to"],
        serde_json::json!(["https://social.example/users/alice/followers"])
    );
    assert_eq!(
        add["object"],
        "https://social.example/users/alice/statuses/123"
    );

    let remove = serde_json::from_str::<serde_json::Value>(
        &build_remove_featured_activity_with_id(
            &config,
            &account,
            "https://social.example/users/alice/statuses/123",
            "https://social.example/users/alice/collections/featured/remove/test",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(remove["type"], "Remove");
    assert_eq!(
        remove["target"],
        "https://social.example/users/alice/collections/featured"
    );
    assert_eq!(
        remove["to"],
        serde_json::json!(["https://social.example/users/alice/followers"])
    );
    assert_eq!(
        remove["object"],
        "https://social.example/users/alice/statuses/123"
    );
}

#[test]
fn follow_targets_local_actor_accepts_string_and_object_forms() {
    assert!(follow_targets_local_actor(
        Some(&serde_json::json!("https://example.com/users/alice")),
        "https://example.com/users/alice",
    ));
    assert!(follow_targets_local_actor(
        Some(&serde_json::json!({"id": "https://example.com/users/alice"})),
        "https://example.com/users/alice",
    ));
    assert!(!follow_targets_local_actor(
        Some(&serde_json::json!("https://example.com/users/bob")),
        "https://example.com/users/alice",
    ));
}

#[test]
fn is_follow_undo_accepts_follow_object_for_same_actor() {
    assert!(is_follow_undo(
        Some(&serde_json::json!({
            "type": "Follow",
            "actor": "https://remote.example/users/bob",
        })),
        "https://remote.example/users/bob",
        "https://remote.example/@bob",
    ));
    assert!(!is_follow_undo(
        Some(&serde_json::json!(
            "https://remote.example/users/bob/statuses/like-1"
        )),
        "https://remote.example/users/bob",
        "https://remote.example/@bob",
    ));
    assert!(!is_follow_undo(
        Some(&serde_json::json!({
            "type": "Like",
            "actor": "https://remote.example/users/bob",
        })),
        "https://remote.example/users/bob",
        "https://remote.example/@bob",
    ));
}

#[test]
fn extract_inbox_target_username_supports_follow_undo_accept_reject_and_create() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Follow",
                "object": "https://social.example/users/alice",
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Accept",
                "object": {
                    "type": "Follow",
                    "actor": "https://social.example/users/alice",
                    "object": "https://remote.example/users/bob"
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Reject",
                "object": {
                    "type": "Follow",
                    "actor": "https://social.example/users/alice",
                    "object": "https://remote.example/users/bob"
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Undo",
                "object": {
                    "type": "Follow",
                    "object": "https://social.example/users/alice",
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Create",
                "object": {
                    "to": ["https://social.example/users/alice"]
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Create",
                "object": {
                    "to": ["https://www.w3.org/ns/activitystreams#Public"],
                    "cc": ["https://social.example/users/alice/followers"]
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Update",
                "object": {
                    "to": ["https://www.w3.org/ns/activitystreams#Public"],
                    "cc": ["https://social.example/users/alice/followers"]
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Like",
                "object": "https://social.example/users/alice/statuses/status-1"
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Undo",
                "object": {
                    "type": "Create",
                    "object": {
                        "type": "Note",
                        "inReplyTo": "https://social.example/users/alice/statuses/status-1"
                    }
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Undo",
                "object": {
                    "type": "Announce",
                    "object": "https://social.example/users/alice/statuses/status-1"
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Announce",
                "object": {
                    "type": "Note",
                    "id": "https://remote.example/users/bob/statuses/quote-1",
                    "quoteUri": "https://social.example/users/alice/statuses/status-1"
                }
            })
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        extract_inbox_target_username(
            &config,
            &serde_json::json!({
                "type": "Announce",
                "object": {
                    "type": "Note",
                    "id": "https://remote.example/users/bob/statuses/quote-2",
                    "to": ["https://social.example/users/alice"]
                }
            })
        ),
        Some("alice".to_owned())
    );
}

#[test]
fn quote_target_uri_from_object_supports_fedibird_and_misskey_fields() {
    assert_eq!(
        quote_target_uri_from_object(&serde_json::json!({
            "quoteUri": "https://remote.example/statuses/1"
        })),
        Some("https://remote.example/statuses/1".to_owned())
    );
    assert_eq!(
        quote_target_uri_from_object(&serde_json::json!({
            "quoteUrl": "https://remote.example/statuses/2"
        })),
        Some("https://remote.example/statuses/2".to_owned())
    );
    assert_eq!(
        quote_target_uri_from_object(&serde_json::json!({
            "_misskey_quote": "https://remote.example/statuses/3"
        })),
        Some("https://remote.example/statuses/3".to_owned())
    );
}

#[test]
fn local_username_from_actor_uri_matches_local_users_only() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert_eq!(
        local_username_from_actor_uri(&config, "https://social.example/users/alice"),
        Some("alice".to_owned())
    );
    assert_eq!(
        local_username_from_actor_uri(&config, "https://remote.example/users/alice"),
        None
    );
    assert_eq!(
        local_username_from_actor_uri(&config, "https://social.example/@alice"),
        Some("alice".to_owned())
    );
}

#[test]
fn local_username_from_status_uri_matches_local_statuses_only() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    assert_eq!(
        local_username_from_status_uri(
            &config,
            "https://social.example/users/alice/statuses/status-1"
        ),
        Some("alice".to_owned())
    );
    assert_eq!(
        local_username_from_status_uri(
            &config,
            "https://remote.example/users/alice/statuses/status-1"
        ),
        None
    );
    assert_eq!(
        local_username_from_status_uri(&config, "https://social.example/@alice/status-1"),
        Some("alice".to_owned())
    );
    assert_eq!(
        local_username_from_status_uri(&config, "https://social.example/@alice/statuses/status-1"),
        Some("alice".to_owned())
    );
}

#[test]
fn visibility_from_activitypub_object_detects_public_and_unlisted() {
    assert_eq!(
        visibility_from_activitypub_object(&serde_json::json!({
            "to": ["https://www.w3.org/ns/activitystreams#Public"]
        })),
        "public"
    );
    assert_eq!(
        visibility_from_activitypub_object(&serde_json::json!({
            "cc": ["https://www.w3.org/ns/activitystreams#Public"]
        })),
        "unlisted"
    );
    assert_eq!(
        visibility_from_activitypub_object(&serde_json::json!({
            "to": "as:Public"
        })),
        "public"
    );
    assert_eq!(
        visibility_from_activitypub_object(&serde_json::json!({
            "to": ["https://social.example/users/alice/followers"]
        })),
        "private"
    );
    assert_eq!(
        visibility_from_activitypub_object(&serde_json::json!({
            "to": ["https://social.example/users/bob"],
            "cc": []
        })),
        "direct"
    );
}

#[test]
fn note_targets_account_or_followers_accepts_public_without_followers_uri() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let object = serde_json::json!({
        "type": "Note",
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": []
    });

    assert!(note_targets_public(&object));
    assert!(note_targets_account_or_followers(&object, &viewer, &config));
    assert!(!note_targets_followers(&object, &viewer, &config));
}

#[test]
fn build_activitypub_delete_uses_status_audience_and_object_id() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = actor_fixture_account();
    let status = LocalStatus {
        id: "status-1".to_owned(),
        account_id: account.id().to_owned(),
        ap_id: None,
        in_reply_to_id: None,
        in_reply_to_account_id: None,
        boost_of_uri: None,
        quote_of_uri: None,
        content_html: "<p>hello</p>".to_owned(),
        text: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: cfwdon_domain::Visibility::Public,
        sensitive: false,
        language: Some("en".to_owned()),
        quote_approval_policy: None,
        quote_state: cfwdon_domain::QuoteState::Accepted,
        application_id: None,
        card_json: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        updated_at: None,
    };

    let note_id = local_status_ap_id(&config, &account, &status);
    let (to, cc) =
        activitypub_audiences_for_visibility(&config, account.username(), status.visibility);
    assert_eq!(
        note_id,
        "https://social.example/users/alice/statuses/status-1"
    );
    assert_eq!(
        to,
        serde_json::json!(["https://www.w3.org/ns/activitystreams#Public"])
    );
    assert_eq!(
        cc,
        serde_json::json!(["https://social.example/users/alice/followers"])
    );
}

#[test]
fn build_status_update_activity_includes_quote_context_when_present() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = actor_fixture_account();
    let object = serde_json::json!({
        "id": "https://social.example/users/alice/statuses/status-1",
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": [],
        "_misskey_quote": "https://remote.example/statuses/quoted"
    });

    let activity = build_status_update_activity_with_id(
        &config,
        &account,
        object,
        "https://social.example/users/alice/statuses/status-1/updates/1",
        "2026-01-02T00:00:00.000Z",
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&activity).unwrap();

    assert!(json["@context"].is_array());
    assert!(
        json["@context"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| {
                entry
                    .get("_misskey_quote")
                    .and_then(|value| value.get("@id"))
                    .and_then(serde_json::Value::as_str)
                    == Some("https://misskey-hub.net/ns#_misskey_quote")
            }))
    );
}

#[test]
fn build_delete_quote_authorization_activity_uses_fep_044f_shape() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = actor_fixture_account();

    let activity = serde_json::from_str::<serde_json::Value>(
        &build_delete_quote_authorization_activity(
            &config,
            &account,
            "https://remote.example/users/bob/statuses/1",
            "https://social.example/users/alice/statuses/2",
            "remote-status-1",
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(activity["type"], serde_json::json!("Delete"));
    assert_eq!(
        activity["actor"],
        serde_json::json!("https://social.example/users/alice")
    );
    assert_eq!(
        activity["object"]["type"],
        serde_json::json!("QuoteAuthorization")
    );
    assert_eq!(
        activity["object"]["attributedTo"],
        serde_json::json!("https://social.example/users/alice")
    );
    assert_eq!(
        activity["object"]["interactingObject"],
        serde_json::json!("https://remote.example/users/bob/statuses/1")
    );
    assert_eq!(
        activity["object"]["interactionTarget"],
        serde_json::json!("https://social.example/users/alice/statuses/2")
    );
    assert_eq!(
        activity["id"],
        serde_json::json!(
            "https://social.example/users/alice/statuses/2/quote_authorizations/remote-status-1#delete"
        )
    );
}

#[test]
fn build_accept_quote_request_activity_uses_fep_044f_shape() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = actor_fixture_account();
    let quote_request = build_quote_request_object(
        "https://remote.example/users/bob/statuses/1/quote_requests/remote-status-1",
        "https://remote.example/users/bob",
        "https://social.example/users/alice/statuses/2",
        "https://remote.example/users/bob/statuses/1",
    );
    let authorization_uri = quote_authorization_uri(
        "https://social.example/users/alice/statuses/2",
        "remote-status-1",
    );

    let activity = serde_json::from_str::<serde_json::Value>(
        &build_accept_quote_request_activity_with_id(
            &config,
            &account,
            &quote_request,
            &authorization_uri,
            "https://remote.example/users/bob",
            "https://social.example/users/alice/accepts/quote_requests/test-1",
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(activity["type"], serde_json::json!("Accept"));
    assert_eq!(
        activity["actor"],
        serde_json::json!("https://social.example/users/alice")
    );
    assert_eq!(
        activity["object"]["type"],
        serde_json::json!("QuoteRequest")
    );
    assert_eq!(activity["result"], serde_json::json!(authorization_uri));
    assert_eq!(
        activity["to"],
        serde_json::json!(["https://remote.example/users/bob"])
    );
}

#[test]
fn build_reject_quote_request_activity_uses_fep_044f_shape() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = actor_fixture_account();
    let quote_request = build_quote_request_object(
        "https://remote.example/users/bob/statuses/1/quote_requests/remote-status-1",
        "https://remote.example/users/bob",
        "https://social.example/users/alice/statuses/2",
        "https://remote.example/users/bob/statuses/1",
    );

    let activity = serde_json::from_str::<serde_json::Value>(
        &build_reject_quote_request_activity_with_id(
            &config,
            &account,
            &quote_request,
            "https://remote.example/users/bob",
            "https://social.example/users/alice/rejects/quote_requests/test-1",
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(activity["type"], serde_json::json!("Reject"));
    assert_eq!(
        activity["actor"],
        serde_json::json!("https://social.example/users/alice")
    );
    assert_eq!(
        activity["object"]["type"],
        serde_json::json!("QuoteRequest")
    );
}

#[test]
fn build_quote_authorization_object_is_dereferenceable_stamp() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = actor_fixture_account();
    let document = build_quote_authorization_object(
        &config,
        &account,
        "https://remote.example/users/bob/statuses/1",
        "https://social.example/users/alice/statuses/2",
        "remote-status-1",
    );

    assert_eq!(document["type"], serde_json::json!("QuoteAuthorization"));
    assert_eq!(
        document["id"],
        serde_json::json!(
            "https://social.example/users/alice/statuses/2/quote_authorizations/remote-status-1"
        )
    );
}

#[test]
fn activitypub_actor_document_reflects_locked_and_bot_flags() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = actor_fixture_account_locked_bot();

    let actor = build_activitypub_actor_document(&config, &account);
    assert_eq!(actor.actor_type, "Service");
    assert!(actor.manually_approves_followers);
    assert_eq!(actor.webfinger, "alice@social.example");
}

#[test]
fn activitypub_media_attachment_type_matches_media_kind() {
    assert_eq!(activitypub_media_attachment_type("image/png"), "Image");
    assert_eq!(activitypub_media_attachment_type("video/mp4"), "Video");
    assert_eq!(activitypub_media_attachment_type("audio/mpeg"), "Audio");
    assert_eq!(
        activitypub_media_attachment_type("application/octet-stream"),
        "Document"
    );
}

#[test]
fn build_update_person_activity_wraps_actor_document() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = actor_fixture_account();

    let activity = serde_json::from_str::<serde_json::Value>(
        &build_update_person_activity_with_id(
            &config,
            &account,
            "https://social.example/users/alice/updates/test-update",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(activity["type"], serde_json::json!("Update"));
    assert_eq!(
        activity["id"],
        serde_json::json!("https://social.example/users/alice/updates/test-update")
    );
    assert_eq!(
        activity["object"]["id"],
        serde_json::json!("https://social.example/users/alice")
    );
    assert_eq!(activity["object"]["discoverable"], serde_json::json!(true));
    assert_eq!(
        activity["object"]["attachment"][0]["name"],
        serde_json::json!("Website")
    );
}

#[test]
fn activitypub_actor_type_detection_matches_supported_profile_types() {
    assert!(is_activitypub_actor_type(Some("Person")));
    assert!(is_activitypub_actor_type(Some("Application")));
    assert!(is_activitypub_actor_type(Some("Group")));
    assert!(!is_activitypub_actor_type(Some("Note")));
    assert!(!is_activitypub_actor_type(None));
}
