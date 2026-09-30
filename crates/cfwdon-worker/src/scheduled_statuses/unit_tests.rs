use crate::scheduled_statuses::{scheduled_status_document, scheduled_status_document_with_params};
use cfwdon_domain::{StatusDraft, Visibility};

#[test]
fn scheduled_status_document_matches_upstream_shape() {
    let document = scheduled_status_document("sched-1");

    assert_eq!(document.pointer("/id"), Some(&serde_json::json!("sched-1")));
    assert_eq!(
        document.pointer("/scheduled_at"),
        Some(&serde_json::json!("2099-01-01T00:00:00.000Z"))
    );
    assert_eq!(
        document.pointer("/params/poll"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(
        document.pointer("/params/text"),
        Some(&serde_json::json!(""))
    );
    assert_eq!(
        document.pointer("/params/application_id"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        document.pointer("/params/with_rate_limit"),
        Some(&serde_json::json!(false))
    );
}

#[test]
fn scheduled_status_document_with_params_reflects_draft_values() {
    let draft = StatusDraft::try_from_persisted(
        "scheduled hello".to_owned(),
        Visibility::Unlisted,
        "cw".to_owned(),
        true,
        Some("ja".to_owned()),
        None,
        Some("status-1".to_owned()),
        vec!["media-1".to_owned()],
        None,
    )
    .unwrap();
    let document =
        scheduled_status_document_with_params("sched-2", "2099-02-03T04:05:06Z", Some(&draft));

    assert_eq!(
        document.pointer("/scheduled_at"),
        Some(&serde_json::json!("2099-02-03T04:05:06Z"))
    );
    assert_eq!(
        document.pointer("/params/text"),
        Some(&serde_json::json!("scheduled hello"))
    );
    assert_eq!(
        document.pointer("/params/visibility"),
        Some(&serde_json::json!("unlisted"))
    );
    assert_eq!(
        document.pointer("/params/media_ids/0"),
        Some(&serde_json::json!("media-1"))
    );
    assert_eq!(
        document.pointer("/params/in_reply_to_id"),
        Some(&serde_json::json!("status-1"))
    );
}
