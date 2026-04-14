use super::{
    CreateStatusPollRequest, MastodonAccountResponse, MastodonReportResponse, RemoteActorRow,
    RemoteStatusPollOptionRow, RemoteStatusPollVoteRow, SearchCategoryFlags, SearchV2Query,
    StatusPollOptionRow, StatusPollRow, StatusRow, TagTimelineQuery,
    activitypub_profile_attachments, apply_activitypub_poll_fields,
    build_activitypub_delete_with_published_at, build_instance_v1_document,
    build_instance_v2_document, build_internal_cursor_link_for_url, build_nodeinfo_document,
    build_nodeinfo_links_document, build_poll_vote_activity_with_ids,
    build_status_update_activity_with_id, build_update_person_activity_with_id,
    classify_media_kind, configured_html_document, delivery_retry_delay_modifier,
    describe_outbound_activity, directory_order, extract_account_handles_from_text,
    extract_hashtags_from_html, extract_hashtags_from_text, extract_inbox_target_username,
    extract_mentions_from_text, extract_remote_note_object, extract_remote_poll_draft,
    extract_remote_profile_media_url, follow_targets_local_actor, include_local_source,
    include_remote_source, instance_base_url, is_activitypub_actor_type, is_admin_account,
    is_follow_undo, local_username_from_actor_uri, local_username_from_status_uri,
    mastodon_account_fields, matches_tag_timeline_filters, media_fallback_url, media_kind_label,
    media_object_url, nodeinfo_url, normalize_status_poll, notification_sort_key,
    notification_timestamp_sort_token, outbound_terminal_failure_follow_state, parse_csv_list,
    parse_http_url_parts, parse_internal_pagination_id, parse_lookup_handle, parse_media_focus,
    parse_remote_actor_profile_document, parse_webfinger_resource, peer_authority_from_uri,
    remap_remote_poll_vote_positions, remote_account_rest_id, remote_actor_uri_from_rest_id,
    resolve_search_tag_name, search_category_flags, search_text_match_rank, search_v2_limit,
    search_v2_requires_auth, tag_search_rank, visibility_from_activitypub_object,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    InstanceCapabilities, InstanceSummary, LocalAccount, ProfileField, SoftwareInfo,
};
use url::Url;

#[test]
fn parse_webfinger_resource_extracts_local_handle() {
    let handle = parse_webfinger_resource("acct:alice@example.com").unwrap();
    assert_eq!(handle.username, "alice");
    assert_eq!(handle.domain.as_deref(), Some("example.com"));
}

#[test]
fn parse_webfinger_resource_rejects_non_acct_scheme() {
    let error = parse_webfinger_resource("https://example.com/users/alice").unwrap_err();
    assert!(error.to_string().contains("acct"));
}

#[test]
fn parse_internal_pagination_id_accepts_integer_cursor() {
    assert_eq!(
        parse_internal_pagination_id(Some("42"), "max_id").unwrap(),
        Some(42)
    );
    assert_eq!(
        parse_internal_pagination_id(Some(""), "max_id").unwrap(),
        None
    );
    assert_eq!(parse_internal_pagination_id(None, "max_id").unwrap(), None);
}

#[test]
fn parse_internal_pagination_id_rejects_invalid_cursor() {
    let error = parse_internal_pagination_id(Some("abc"), "since_id").unwrap_err();
    assert!(error.to_string().contains("since_id"));
}

#[test]
fn internal_cursor_link_header_preserves_other_query_params() {
    let url = Url::parse("https://social.example/api/v1/mutes?foo=bar&limit=20").unwrap();
    let next = build_internal_cursor_link_for_url(&url, 10, Some(150), None, "next").unwrap();
    let prev = build_internal_cursor_link_for_url(&url, 10, None, Some(200), "prev").unwrap();

    assert!(next.contains("foo=bar"));
    assert!(next.contains("limit=10"));
    assert!(next.contains("max_id=150"));
    assert!(next.contains("rel=\"next\""));
    assert!(prev.contains("foo=bar"));
    assert!(prev.contains("limit=10"));
    assert!(prev.contains("since_id=200"));
    assert!(prev.contains("rel=\"prev\""));
}

#[test]
fn describe_outbound_activity_extracts_id_and_type() {
    let descriptor = describe_outbound_activity(
        r#"{"id":"https://social.example/users/alice/likes/123","type":"Like"}"#,
    )
    .unwrap();

    assert_eq!(
        descriptor.activity_id,
        "https://social.example/users/alice/likes/123"
    );
    assert_eq!(descriptor.activity_type, "Like");
}

#[test]
fn describe_outbound_activity_rejects_missing_fields() {
    assert!(describe_outbound_activity(r#"{"type":"Like"}"#).is_err());
    assert!(describe_outbound_activity(r#"{"id":"abc"}"#).is_err());
}

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
fn extract_remote_poll_draft_reads_question_options_and_counts() {
    let question = serde_json::json!({
        "type":"Question",
        "endTime":"2026-03-01T00:00:00Z",
        "votersCount": 2,
        "anyOf":[
            {"type":"Note","name":"rust","replies":{"totalItems":2}},
            {"type":"Note","name":"workers","replies":{"totalItems":1}}
        ]
    });

    let poll = extract_remote_poll_draft(&question).unwrap();
    assert!(poll.multiple);
    assert_eq!(poll.expires_at.as_deref(), Some("2026-03-01T00:00:00Z"));
    assert_eq!(poll.voters_count, Some(2));
    assert_eq!(poll.votes_count, 3);
    assert_eq!(poll.options.len(), 2);
    assert_eq!(poll.options[0].title, "rust");
    assert_eq!(poll.options[1].votes_count, 1);
}

#[test]
fn remap_remote_poll_vote_positions_prefers_matching_title_after_reorder() {
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "green".to_owned(),
            votes_count: 5,
        },
        RemoteStatusPollOptionRow {
            title: "orange".to_owned(),
            votes_count: 3,
        },
        RemoteStatusPollOptionRow {
            title: "blue".to_owned(),
            votes_count: 1,
        },
    ];
    let votes = vec![RemoteStatusPollVoteRow {
        option_position: 0,
        option_title: Some("orange".to_owned()),
    }];

    assert_eq!(remap_remote_poll_vote_positions(&options, &votes), vec![1]);
}

#[test]
fn remap_remote_poll_vote_positions_falls_back_to_stored_position_for_legacy_rows() {
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "yes".to_owned(),
            votes_count: 2,
        },
        RemoteStatusPollOptionRow {
            title: "no".to_owned(),
            votes_count: 1,
        },
    ];
    let votes = vec![RemoteStatusPollVoteRow {
        option_position: 1,
        option_title: None,
    }];

    assert_eq!(remap_remote_poll_vote_positions(&options, &votes), vec![1]);
}

#[test]
fn remap_remote_poll_vote_positions_drops_unresolvable_stale_votes() {
    let options = vec![RemoteStatusPollOptionRow {
        title: "green".to_owned(),
        votes_count: 2,
    }];
    let votes = vec![RemoteStatusPollVoteRow {
        option_position: 4,
        option_title: Some("orange".to_owned()),
    }];

    assert!(remap_remote_poll_vote_positions(&options, &votes).is_empty());
}

#[test]
fn build_poll_vote_activity_uses_question_reply_shape() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    let (activity_id, payload) = build_poll_vote_activity_with_ids(
        &config,
        &account,
        "https://remote.example/users/bob",
        "https://remote.example/questions/1",
        "orange",
        "https://social.example/users/alice/votes/test-vote",
        "https://social.example/users/alice/votes/test-vote/activity",
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&payload).unwrap();
    assert_eq!(value["id"], serde_json::json!(activity_id));
    assert_eq!(value["type"], serde_json::json!("Create"));
    assert_eq!(
        value["to"],
        serde_json::json!(["https://remote.example/users/bob"])
    );
    assert_eq!(
        value["object"]["inReplyTo"],
        serde_json::json!("https://remote.example/questions/1")
    );
    assert_eq!(value["object"]["name"], serde_json::json!("orange"));
}

#[test]
fn build_status_update_activity_wraps_question_object() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
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
fn apply_activitypub_poll_fields_uses_question_shape_for_single_choice() {
    let poll = StatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 0,
        hide_totals: 0,
        expires_at: "2026-02-01T00:00:00Z".to_owned(),
    };
    let options = vec![
        StatusPollOptionRow {
            title: "yes".to_owned(),
            votes_count: 2,
        },
        StatusPollOptionRow {
            title: "no".to_owned(),
            votes_count: 1,
        },
    ];
    let mut object = serde_json::json!({
        "type": "Note",
        "id": "https://social.example/users/alice/statuses/status-1",
    });

    apply_activitypub_poll_fields(&mut object, &poll, &options, 3, true);
    assert_eq!(object["type"], serde_json::json!("Question"));
    assert_eq!(object["endTime"], serde_json::json!("2026-02-01T00:00:00Z"));
    assert_eq!(object["closed"], serde_json::json!("2026-02-01T00:00:00Z"));
    assert_eq!(object["votersCount"], serde_json::json!(3));
    assert!(object.get("anyOf").is_none());
    assert_eq!(object["oneOf"][0]["name"], serde_json::json!("yes"));
    assert_eq!(
        object["oneOf"][1]["replies"]["totalItems"],
        serde_json::json!(1)
    );
}

#[test]
fn apply_activitypub_poll_fields_uses_any_of_for_multiple_choice() {
    let poll = StatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 1,
        hide_totals: 0,
        expires_at: "2026-02-01T00:00:00Z".to_owned(),
    };
    let options = vec![
        StatusPollOptionRow {
            title: "rust".to_owned(),
            votes_count: 2,
        },
        StatusPollOptionRow {
            title: "workers".to_owned(),
            votes_count: 3,
        },
    ];
    let mut object = serde_json::json!({
        "type": "Note",
        "id": "https://social.example/users/alice/statuses/status-1",
    });

    apply_activitypub_poll_fields(&mut object, &poll, &options, 4, false);
    assert_eq!(object["type"], serde_json::json!("Question"));
    assert!(object.get("oneOf").is_none());
    assert_eq!(object["anyOf"][0]["name"], serde_json::json!("rust"));
    assert_eq!(
        object["anyOf"][1]["replies"]["totalItems"],
        serde_json::json!(3)
    );
    assert!(object.get("closed").is_none());
}

#[test]
fn outbound_terminal_failure_marks_follow_as_failed_only_for_follow() {
    assert_eq!(
        outbound_terminal_failure_follow_state("Follow"),
        Some("failed")
    );
    assert_eq!(outbound_terminal_failure_follow_state("Undo"), None);
    assert_eq!(outbound_terminal_failure_follow_state("Like"), None);
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
fn classify_media_kind_detects_supported_types() {
    assert_eq!(
        classify_media_kind("image/png").map(media_kind_label),
        Some("image")
    );
    assert_eq!(
        classify_media_kind("video/mp4").map(media_kind_label),
        Some("video")
    );
    assert_eq!(
        classify_media_kind("audio/ogg").map(media_kind_label),
        Some("audio")
    );
    assert_eq!(classify_media_kind("application/pdf"), None);
}

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
fn delivery_retry_delay_backoff_steps_up() {
    assert_eq!(delivery_retry_delay_modifier(1), "+1 minute");
    assert_eq!(delivery_retry_delay_modifier(2), "+5 minutes");
    assert_eq!(delivery_retry_delay_modifier(3), "+15 minutes");
    assert_eq!(delivery_retry_delay_modifier(4), "+60 minutes");
    assert_eq!(delivery_retry_delay_modifier(8), "+60 minutes");
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
        None
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
            "to": ["https://social.example/users/alice/followers"]
        })),
        "private"
    );
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
fn search_category_flags_defaults_to_all_categories() {
    assert_eq!(
        search_category_flags(None),
        SearchCategoryFlags {
            accounts: true,
            statuses: true,
            hashtags: true,
        }
    );
}

#[test]
fn search_category_flags_respects_explicit_type() {
    assert_eq!(
        search_category_flags(Some("accounts")),
        SearchCategoryFlags {
            accounts: true,
            statuses: false,
            hashtags: false,
        }
    );
    assert_eq!(
        search_category_flags(Some("statuses")),
        SearchCategoryFlags {
            accounts: false,
            statuses: true,
            hashtags: false,
        }
    );
    assert_eq!(
        search_category_flags(Some("hashtags")),
        SearchCategoryFlags {
            accounts: false,
            statuses: false,
            hashtags: true,
        }
    );
}

#[test]
fn search_v2_requires_auth_for_resolve_following_and_offset() {
    assert!(search_v2_requires_auth(&SearchV2Query {
        resolve: Some(true),
        ..SearchV2Query::default()
    }));
    assert!(search_v2_requires_auth(&SearchV2Query {
        following: Some(true),
        ..SearchV2Query::default()
    }));
    assert!(search_v2_requires_auth(&SearchV2Query {
        offset: Some(1),
        ..SearchV2Query::default()
    }));
    assert!(!search_v2_requires_auth(&SearchV2Query::default()));
}

#[test]
fn search_v2_limit_matches_mastodon_bounds() {
    assert_eq!(search_v2_limit(None), 20);
    assert_eq!(search_v2_limit(Some(0)), 1);
    assert_eq!(search_v2_limit(Some(5)), 5);
    assert_eq!(search_v2_limit(Some(80)), 40);
}

#[test]
fn search_text_match_rank_prefers_exact_then_prefix_then_contains() {
    assert_eq!(search_text_match_rank("alice", "alice"), 0);
    assert_eq!(search_text_match_rank("ali", "alice"), 1);
    assert_eq!(search_text_match_rank("lic", "alice"), 2);
    assert_eq!(search_text_match_rank("bob", "alice"), 3);
}

#[test]
fn tag_search_rank_prefers_exact_matches() {
    assert!(tag_search_rank("rust", "rust") < tag_search_rank("rust", "rustlang"));
    assert!(tag_search_rank("rust", "rustlang") < tag_search_rank("rust", "fedirust"));
}

#[test]
fn resolve_search_tag_name_supports_hash_and_tag_urls() {
    assert_eq!(resolve_search_tag_name("#Rust"), Some("rust".to_owned()));
    assert_eq!(
        resolve_search_tag_name("https://social.example/tags/Rust"),
        Some("rust".to_owned())
    );
    assert_eq!(
        resolve_search_tag_name("https://social.example/explore/tags/Workers"),
        Some("workers".to_owned())
    );
    assert_eq!(
        resolve_search_tag_name("/tags/fediverse_test"),
        Some("fediverse_test".to_owned())
    );
}

#[test]
fn resolve_search_tag_name_rejects_non_tag_queries() {
    assert_eq!(resolve_search_tag_name("rust"), None);
    assert_eq!(
        resolve_search_tag_name("https://social.example/@alice"),
        None
    );
    assert_eq!(resolve_search_tag_name(""), None);
}

#[test]
fn extract_hashtags_from_text_deduplicates_and_normalizes() {
    assert_eq!(
        extract_hashtags_from_text("Hello #Rust #rust and #fediverse_test"),
        vec!["rust".to_owned(), "fediverse_test".to_owned()]
    );
}

#[test]
fn extract_hashtags_from_html_ignores_markup() {
    assert_eq!(
        extract_hashtags_from_html(
            "<p><a href=\"https://example/tags/rust\">#<span>Rust</span></a> and #Workers</p>"
        ),
        vec!["rust".to_owned(), "workers".to_owned()]
    );
}

#[test]
fn extract_mentions_from_text_finds_local_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_mentions_from_text(
        "@alice hi @bob@social.example and @carol@remote.example",
        &config,
    );
    assert_eq!(mentions.len(), 2);
    assert_eq!(mentions[0].username, "alice");
    assert_eq!(mentions[1].username, "bob");
}

#[test]
fn extract_mentions_from_text_deduplicates_local_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_mentions_from_text("@alice @alice@social.example", &config);
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].username, "alice");
}

#[test]
fn extract_account_handles_from_text_keeps_remote_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_account_handles_from_text("@alice @bob@remote.example @alice", &config);
    assert_eq!(mentions.len(), 2);
    assert_eq!(mentions[0].username, "alice");
    assert_eq!(mentions[0].domain.as_deref(), Some("social.example"));
    assert_eq!(mentions[1].username, "bob");
    assert_eq!(mentions[1].domain.as_deref(), Some("remote.example"));
}

#[test]
fn build_activitypub_delete_uses_status_audience_and_object_id() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
    let status = StatusRow {
        id: "status-1".to_owned(),
        account_id: account.id.clone(),
        ap_id: None,
        in_reply_to_id: None,
        content_html: "<p>hello</p>".to_owned(),
        _text_content: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: "public".to_owned(),
        sensitive: 0,
        language: Some("en".to_owned()),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

    let activity = build_activitypub_delete_with_published_at(
        &config,
        &account,
        &status,
        "2026-01-02T00:00:00.000Z",
    )
    .unwrap();
    assert_eq!(activity.get("type"), Some(&serde_json::json!("Delete")));
    assert_eq!(
        activity.get("object"),
        Some(&serde_json::json!(
            "https://social.example/users/alice/statuses/status-1"
        ))
    );
    assert_eq!(
        activity.get("published"),
        Some(&serde_json::json!("2026-01-02T00:00:00.000Z"))
    );
    assert_eq!(
        activity.get("to"),
        Some(&serde_json::json!([
            "https://www.w3.org/ns/activitystreams#Public"
        ]))
    );
    assert_eq!(
        activity.pointer("/cc/0"),
        Some(&serde_json::json!(
            "https://social.example/users/alice/followers"
        ))
    );
}

#[test]
fn matches_tag_timeline_filters_applies_any_all_none() {
    let tags = vec![
        "rust".to_owned(),
        "workers".to_owned(),
        "activitypub".to_owned(),
    ];
    assert!(matches_tag_timeline_filters(
        &tags,
        "rust",
        &TagTimelineQuery::default()
    ));
    assert!(matches_tag_timeline_filters(
        &tags,
        "rust",
        &TagTimelineQuery {
            any: Some(vec!["workers".to_owned(), "d1".to_owned()]),
            all: Some(vec!["activitypub".to_owned()]),
            ..TagTimelineQuery::default()
        }
    ));
    assert!(!matches_tag_timeline_filters(
        &tags,
        "rust",
        &TagTimelineQuery {
            none: Some(vec!["workers".to_owned()]),
            ..TagTimelineQuery::default()
        }
    ));
}

#[test]
fn tag_timeline_source_flags_default_to_both_sources() {
    assert!(include_local_source(None, None));
    assert!(include_remote_source(None, None));
    assert!(include_local_source(Some(true), Some(false)));
    assert!(!include_remote_source(Some(true), Some(false)));
    assert!(!include_local_source(Some(false), Some(true)));
    assert!(include_remote_source(Some(false), Some(true)));
}

#[test]
fn parse_media_focus_accepts_valid_coordinates() {
    assert_eq!(
        parse_media_focus(Some("0.25,-0.5")).unwrap(),
        Some((0.25, -0.5))
    );
    assert_eq!(parse_media_focus(Some("")).unwrap(), None);
    assert_eq!(parse_media_focus(None).unwrap(), None);
}

#[test]
fn parse_media_focus_rejects_invalid_coordinates() {
    assert!(parse_media_focus(Some("1.5,0")).is_err());
    assert!(parse_media_focus(Some("abc,0")).is_err());
    assert!(parse_media_focus(Some("0")).is_err());
}

#[test]
fn media_urls_prefer_custom_domain_and_keep_worker_fallback() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.media_public_base_url = Some("https://media.example.com".to_owned());
    assert_eq!(
        media_object_url(&config, "media/account/image/abc"),
        "https://media.example.com/media/account/image/abc"
    );
    assert_eq!(
        media_fallback_url(&config, "abc"),
        "https://social.example/media/abc"
    );
}

#[test]
fn mastodon_report_response_serializes_forwarded_and_nullable_status_ids() {
    let target_account = MastodonAccountResponse {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        acct: "alice".to_owned(),
        display_name: "Alice".to_owned(),
        locked: false,
        bot: false,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        note: String::new(),
        url: "https://social.example/@alice".to_owned(),
        avatar: String::new(),
        avatar_static: String::new(),
        header: String::new(),
        header_static: String::new(),
        fields: Vec::new(),
        followers_count: 0,
        following_count: 0,
        statuses_count: 0,
        source: None,
    };
    let response = MastodonReportResponse {
        id: "report-1".to_owned(),
        action_taken: false,
        action_taken_at: None,
        category: "other".to_owned(),
        comment: "context".to_owned(),
        forwarded: false,
        created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        status_ids: None,
        target_account,
        rule_ids: None,
    };

    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(value["forwarded"], serde_json::json!(false));
    assert!(value.get("forward").is_none());
    assert_eq!(value["status_ids"], serde_json::Value::Null);
    assert_eq!(value["rule_ids"], serde_json::Value::Null);
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
fn remote_account_response_uses_cached_profile_media() {
    let actor = RemoteActorRow {
        actor_uri: "https://remote.example/users/alice".to_owned(),
        username: "alice".to_owned(),
        domain: "remote.example".to_owned(),
        display_name: "Alice".to_owned(),
        summary_html: "<p>hello</p>".to_owned(),
        profile_url: Some("https://remote.example/@alice".to_owned()),
        avatar_url: Some("https://cdn.remote.example/avatar.png".to_owned()),
        header_url: Some("https://cdn.remote.example/header.png".to_owned()),
    };

    let response = MastodonAccountResponse::from_remote_actor(&actor);
    assert_eq!(response.avatar, "https://cdn.remote.example/avatar.png");
    assert_eq!(response.header, "https://cdn.remote.example/header.png");
    assert_eq!(response.url, "https://remote.example/@alice");
}

#[test]
fn mastodon_account_fields_render_urls_as_links() {
    let fields = vec![ProfileField {
        name: "Website".to_owned(),
        value: "https://example.com".to_owned(),
    }];
    let rendered = mastodon_account_fields(&fields);
    assert_eq!(rendered[0]["name"], serde_json::json!("Website"));
    assert!(
        rendered[0]["value"]
            .as_str()
            .unwrap_or_default()
            .contains("<a href=\"https://example.com\"")
    );
}

#[test]
fn activitypub_profile_attachments_use_property_value_shape() {
    let fields = vec![ProfileField {
        name: "Pronouns".to_owned(),
        value: "they/them".to_owned(),
    }];
    let rendered = activitypub_profile_attachments(&fields);
    assert_eq!(rendered[0]["type"], serde_json::json!("PropertyValue"));
    assert_eq!(rendered[0]["name"], serde_json::json!("Pronouns"));
    assert_eq!(rendered[0]["value"], serde_json::json!("they/them"));
}

#[test]
fn build_update_person_activity_wraps_actor_document() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "alice@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: "<p>hello</p>".to_owned(),
        bio_text: "hello".to_owned(),
        fields: vec![ProfileField {
            name: "Website".to_owned(),
            value: "https://example.com".to_owned(),
        }],
        discoverable: true,
        default_post_visibility: "public".to_owned(),
        default_sensitive: false,
        default_language: Some("en".to_owned()),
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };

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
fn parse_remote_actor_profile_document_extracts_profile_fields() {
    let actor = serde_json::json!({
        "id": "https://remote.example/users/alice",
        "type": "Person",
        "preferredUsername": "Alice",
        "name": "Alice Example",
        "summary": "<p>remote bio</p>",
        "inbox": "https://remote.example/users/alice/inbox",
        "endpoints": {
            "sharedInbox": "https://remote.example/inbox"
        },
        "publicKey": {
            "id": "https://remote.example/users/alice#main-key",
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
}

#[test]
fn activitypub_actor_type_detection_matches_supported_profile_types() {
    assert!(is_activitypub_actor_type(Some("Person")));
    assert!(is_activitypub_actor_type(Some("Application")));
    assert!(is_activitypub_actor_type(Some("Group")));
    assert!(!is_activitypub_actor_type(Some("Note")));
    assert!(!is_activitypub_actor_type(None));
}

#[test]
fn normalize_status_poll_accepts_minimal_valid_poll() {
    let poll = normalize_status_poll(Some(CreateStatusPollRequest {
        options: Some(vec![" One ".to_owned(), "Two".to_owned(), String::new()]),
        expires_in: Some(600),
        multiple: Some(true),
        hide_totals: Some(true),
    }))
    .unwrap()
    .unwrap();

    assert_eq!(poll.options, vec!["One".to_owned(), "Two".to_owned()]);
    assert_eq!(poll.expires_in_seconds, 600);
    assert!(poll.multiple);
    assert!(poll.hide_totals);
}

#[test]
fn normalize_status_poll_rejects_invalid_shapes() {
    assert!(
        normalize_status_poll(Some(CreateStatusPollRequest {
            options: Some(vec!["Only one".to_owned()]),
            expires_in: Some(600),
            multiple: None,
            hide_totals: None,
        }))
        .is_err()
    );
    assert!(
        normalize_status_poll(Some(CreateStatusPollRequest {
            options: Some(vec!["One".to_owned(), "Two".to_owned()]),
            expires_in: Some(60),
            multiple: None,
            hide_totals: None,
        }))
        .is_err()
    );
}

#[test]
fn is_admin_account_matches_configured_emails() {
    let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    config.admin_emails = vec!["admin@example.com".to_owned()];
    let mut account = LocalAccount {
        id: "acct-1".to_owned(),
        username: "alice".to_owned(),
        access_email: "admin@example.com".to_owned(),
        display_name: "Alice".to_owned(),
        bio_html: String::new(),
        bio_text: String::new(),
        fields: Vec::new(),
        discoverable: false,
        default_post_visibility: "public".to_owned(),
        default_sensitive: false,
        default_language: None,
        avatar_object_key: None,
        avatar_content_type: None,
        header_object_key: None,
        header_content_type: None,
        private_key_jwk: "{}".to_owned(),
        public_key_pem: "pem".to_owned(),
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
    };
    assert!(is_admin_account(&config, &account));

    account.access_email = "user@example.com".to_owned();
    assert!(!is_admin_account(&config, &account));
}

#[test]
fn directory_order_defaults_to_active_and_accepts_new() {
    assert_eq!(directory_order(None), super::DirectoryOrder::Active);
    assert_eq!(
        directory_order(Some("active")),
        super::DirectoryOrder::Active
    );
    assert_eq!(directory_order(Some("new")), super::DirectoryOrder::New);
    assert_eq!(directory_order(Some("NEW")), super::DirectoryOrder::New);
    assert_eq!(
        directory_order(Some("unexpected")),
        super::DirectoryOrder::Active
    );
}

#[test]
fn parse_csv_list_normalizes_and_deduplicates() {
    assert_eq!(
        parse_csv_list("Ja, en,ja ,, EN"),
        vec!["en".to_owned(), "ja".to_owned()]
    );
}

#[test]
fn notification_timestamp_sort_token_supports_sqlite_and_iso_shapes() {
    assert!(notification_timestamp_sort_token("2026-04-14 12:34:56").is_some());
    assert!(notification_timestamp_sort_token("2026-04-14T12:34:56.000Z").is_some());
    assert!(notification_timestamp_sort_token("not-a-date").is_none());
}

#[test]
fn notification_sort_key_orders_newer_timestamps_higher() {
    assert!(
        notification_sort_key("2026-04-14T12:34:56.000Z")
            > notification_sort_key("2026-04-14 12:33:56")
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
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        document.pointer("/configuration/polls/max_options"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        document.pointer("/registrations/enabled"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        document.pointer("/contact/email"),
        Some(&serde_json::json!("admin@example.com"))
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

    let document = build_nodeinfo_document(&summary, &config, 5, 3, 8);
    assert_eq!(document["protocols"][0], serde_json::json!("activitypub"));
    assert_eq!(document["usage"]["users"]["total"], serde_json::json!(5));
    assert_eq!(
        document["usage"]["users"]["activeMonth"],
        serde_json::json!(3)
    );
    assert_eq!(document["usage"]["localPosts"], serde_json::json!(8));
}

#[test]
fn configured_html_document_builds_privacy_and_terms_shapes() {
    let privacy = configured_html_document(
        Some("<p>Privacy</p>"),
        Some("2026-01-01T00:00:00Z"),
        "1970-01-01T00:00:00Z",
        false,
    )
    .unwrap();
    assert_eq!(
        privacy,
        serde_json::json!({
            "updated_at": "2026-01-01T00:00:00Z",
            "content": "<p>Privacy</p>",
        })
    );

    let terms =
        configured_html_document(Some("<p>Terms</p>"), Some("2026-02-01"), "1970-01-01", true)
            .unwrap();
    assert_eq!(
        terms,
        serde_json::json!({
            "effective_date": "2026-02-01",
            "effective": true,
            "content": "<p>Terms</p>",
            "succeeded_by": serde_json::Value::Null,
        })
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
