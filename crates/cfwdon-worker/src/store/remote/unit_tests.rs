use crate::responses::MastodonAccountResponse;
use crate::store::remote::{
    RemoteActorRow, RemoteStatusPollOptionRow, RemoteStatusPollVoteRow,
    remap_remote_poll_vote_positions,
};

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
fn remote_account_response_uses_cached_profile_media() {
    let actor = RemoteActorRow {
        actor_uri: "https://remote.example/users/alice".to_owned(),
        username: "alice".to_owned(),
        domain: "remote.example".to_owned(),
        created_at: "2026-01-02 03:04:05".to_owned(),
        locked: true,
        bot: true,
        discoverable: false,
        indexable: false,
        display_name: "Alice".to_owned(),
        summary_html: "<p>hello</p>".to_owned(),
        profile_url: Some("https://remote.example/@alice".to_owned()),
        avatar_url: Some("https://cdn.remote.example/avatar.png".to_owned()),
        header_url: Some("https://cdn.remote.example/header.png".to_owned()),
        followers_count: 0,
        following_count: 0,
        statuses_count: 0,
        social_counts_updated_at: None,
    };

    let response = MastodonAccountResponse::from_remote_actor(&actor);
    assert_eq!(response.avatar, "https://cdn.remote.example/avatar.png");
    assert_eq!(response.header, "https://cdn.remote.example/header.png");
    assert_eq!(response.url, "https://remote.example/@alice");
    assert_eq!(response.created_at, "2026-01-02T00:00:00.000Z");
    assert!(response.locked);
    assert!(response.bot);
    assert!(!response.discoverable);
    assert!(!response.indexable);
}

#[test]
fn remote_account_response_uses_valid_created_at_fallback() {
    let actor = RemoteActorRow {
        actor_uri: "https://remote.example/users/bob".to_owned(),
        username: "bob".to_owned(),
        domain: "remote.example".to_owned(),
        created_at: String::new(),
        locked: false,
        bot: false,
        discoverable: true,
        indexable: true,
        display_name: "Bob".to_owned(),
        summary_html: String::new(),
        profile_url: None,
        avatar_url: None,
        header_url: None,
        followers_count: 0,
        following_count: 0,
        statuses_count: 0,
        social_counts_updated_at: None,
    };

    let response = MastodonAccountResponse::from_remote_actor(&actor);
    assert_eq!(response.created_at, "1970-01-01T00:00:00.000Z");
}
