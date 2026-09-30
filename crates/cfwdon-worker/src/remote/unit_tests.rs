use crate::activitypub::note_targets_account_or_followers;
use crate::remote::{
    RemotePollDraft, RemotePollOptionDraft, build_poll_vote_activity_with_ids,
    effective_remote_status_quote_state, extract_remote_poll_draft,
    optimistic_remote_poll_vote_deltas, remote_poll_draft_acknowledges_local_snapshot,
    remote_poll_draft_acknowledges_vote, remote_poll_should_refresh,
    remote_status_targets_local_viewer, remote_status_targets_local_viewer_account,
    remote_status_targets_local_viewer_followers,
};
use crate::store::remote::{RemoteStatusPollOptionRow, RemoteStatusPollRow};
use crate::test_fixtures::actor_fixture_account;
use cfwdon_core::AppConfig;
use cfwdon_domain::{QuoteState, RemoteStatus, Visibility};

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
fn optimistic_remote_poll_vote_deltas_increment_multi_voter_once() {
    assert_eq!(
        optimistic_remote_poll_vote_deltas(true, false, 2),
        (2, Some(1))
    );
    assert_eq!(optimistic_remote_poll_vote_deltas(true, true, 1), (1, None));
}

#[test]
fn optimistic_remote_poll_vote_deltas_do_not_set_single_choice_voters_count() {
    assert_eq!(
        optimistic_remote_poll_vote_deltas(false, false, 1),
        (1, None)
    );
}

#[test]
fn remote_poll_draft_acknowledges_vote_accepts_matching_or_newer_totals() {
    let poll = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 1,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(2),
        votes_count: 3,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "rust".to_owned(),
            votes_count: 2,
        },
        RemoteStatusPollOptionRow {
            title: "workers".to_owned(),
            votes_count: 1,
        },
    ];
    let fetched = RemotePollDraft {
        multiple: true,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(3),
        votes_count: 4,
        expired: false,
        options: vec![
            RemotePollOptionDraft {
                title: "rust".to_owned(),
                votes_count: 3,
            },
            RemotePollOptionDraft {
                title: "workers".to_owned(),
                votes_count: 1,
            },
        ],
    };

    assert!(remote_poll_draft_acknowledges_vote(
        &poll,
        &options,
        &fetched,
        false,
        &[0]
    ));
}

#[test]
fn remote_poll_draft_acknowledges_vote_rejects_stale_totals() {
    let poll = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 0,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(3),
        votes_count: 3,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
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
    let fetched = RemotePollDraft {
        multiple: false,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(3),
        votes_count: 3,
        expired: false,
        options: vec![
            RemotePollOptionDraft {
                title: "yes".to_owned(),
                votes_count: 2,
            },
            RemotePollOptionDraft {
                title: "no".to_owned(),
                votes_count: 1,
            },
        ],
    };

    assert!(!remote_poll_draft_acknowledges_vote(
        &poll,
        &options,
        &fetched,
        false,
        &[0]
    ));
}

#[test]
fn remote_poll_draft_acknowledges_local_snapshot_accepts_matching_or_newer_totals() {
    let poll = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 1,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(4),
        votes_count: 6,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let options = vec![
        RemoteStatusPollOptionRow {
            title: "rust".to_owned(),
            votes_count: 4,
        },
        RemoteStatusPollOptionRow {
            title: "workers".to_owned(),
            votes_count: 2,
        },
    ];
    let fetched = RemotePollDraft {
        multiple: true,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(5),
        votes_count: 7,
        expired: false,
        options: vec![
            RemotePollOptionDraft {
                title: "rust".to_owned(),
                votes_count: 4,
            },
            RemotePollOptionDraft {
                title: "workers".to_owned(),
                votes_count: 3,
            },
        ],
    };

    assert!(remote_poll_draft_acknowledges_local_snapshot(
        &poll, &options, &fetched
    ));
}

#[test]
fn remote_poll_draft_acknowledges_local_snapshot_rejects_stale_option_totals() {
    let poll = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 0,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(3),
        votes_count: 3,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
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
    let fetched = RemotePollDraft {
        multiple: false,
        expires_at: Some("2026-03-01T00:00:00Z".to_owned()),
        voters_count: Some(4),
        votes_count: 4,
        expired: false,
        options: vec![
            RemotePollOptionDraft {
                title: "yes".to_owned(),
                votes_count: 1,
            },
            RemotePollOptionDraft {
                title: "no".to_owned(),
                votes_count: 3,
            },
        ],
    };

    assert!(!remote_poll_draft_acknowledges_local_snapshot(
        &poll, &options, &fetched
    ));
}

#[test]
fn build_poll_vote_activity_uses_question_reply_shape() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let account = actor_fixture_account();

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
fn remote_poll_should_refresh_only_for_signed_in_active_polls() {
    let active = RemoteStatusPollRow {
        id: "poll-1".to_owned(),
        status_id: "status-1".to_owned(),
        multiple: 0,
        expires_at: None,
        voters_count: None,
        votes_count: 0,
        expired: 0,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let expired = RemoteStatusPollRow {
        id: "poll-2".to_owned(),
        status_id: "status-2".to_owned(),
        multiple: 0,
        expires_at: None,
        voters_count: None,
        votes_count: 0,
        expired: 1,
        updated_at: "2026-01-01 00:00:00".to_owned(),
    };
    let viewer = actor_fixture_account();

    assert!(remote_poll_should_refresh(&active, Some(&viewer)));
    assert!(!remote_poll_should_refresh(&active, None));
    assert!(!remote_poll_should_refresh(&expired, Some(&viewer)));
}

#[test]
fn remote_status_targets_local_viewer_matches_direct_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/alice"],
        "cc": []
    });

    assert!(remote_status_targets_local_viewer(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_rejects_other_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/bob"],
        "cc": []
    });

    assert!(!remote_status_targets_local_viewer(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_account_rejects_followers_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/alice/followers"],
        "cc": []
    });

    assert!(!remote_status_targets_local_viewer_account(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_followers_matches_followers_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/alice/followers"],
        "cc": []
    });

    assert!(remote_status_targets_local_viewer_followers(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_followers_rejects_direct_audience() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Question",
        "to": ["https://social.example/users/alice"],
        "cc": []
    });

    assert!(!remote_status_targets_local_viewer_followers(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_targets_local_viewer_followers_matches_remote_author_followers() {
    let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
    let viewer = actor_fixture_account();
    let raw_status = serde_json::json!({
        "type": "Note",
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": ["https://misskey.io/users/9jc6bgzfkw/followers"]
    });

    assert!(remote_status_targets_local_viewer_followers(
        &raw_status,
        &viewer,
        &config
    ));
    assert!(note_targets_account_or_followers(
        &raw_status,
        &viewer,
        &config
    ));
}

#[test]
fn remote_status_quote_helpers_follow_quote_state() {
    let mut status = RemoteStatus {
        id: "remote-1".to_owned(),
        actor_uri: "https://remote.example/users/bob".to_owned(),
        object_uri: "https://remote.example/users/bob/statuses/1".to_owned(),
        url: Some("https://remote.example/@bob/1".to_owned()),
        in_reply_to_uri: None,
        boost_of_uri: None,
        quote_of_uri: Some("https://social.example/users/alice/statuses/1".to_owned()),
        content_html: "<p>hello</p>".to_owned(),
        text_content: "hello".to_owned(),
        spoiler_text: String::new(),
        visibility: Visibility::Public,
        sensitive: false,
        language: Some("en".to_owned()),
        quote_state: QuoteState::Accepted,
        published_at: "2026-01-01T00:00:00.000Z".to_owned(),
        edited_at: None,
        card_json: None,
        federated_emojis_json: "[]".to_owned(),
        in_reply_to_id: None,
        interaction_counts: None,
    };

    assert_eq!(effective_remote_status_quote_state(&status), "accepted");
    assert!(status.has_active_quote());

    status.quote_state = QuoteState::Revoked;
    assert_eq!(effective_remote_status_quote_state(&status), "revoked");
    assert!(!status.has_active_quote());
}
