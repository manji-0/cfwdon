use crate::local_polls::apply_activitypub_poll_fields;
use crate::store::local_polls::{StatusPollOptionRow, StatusPollRow};

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
