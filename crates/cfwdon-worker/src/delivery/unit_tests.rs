use crate::delivery::{
    OutboxProcessResponse, describe_outbound_activity, outbox_batch_made_progress,
    request_may_enqueue_outbox_work,
};

#[test]
fn outbox_kick_covers_successful_mutating_requests() {
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        assert!(request_may_enqueue_outbox_work(
            method,
            "/api/v1/accounts/account-1/follow",
            200
        ));
    }
    assert!(request_may_enqueue_outbox_work(
        "POST",
        "/users/alice/inbox",
        202
    ));
    assert!(request_may_enqueue_outbox_work(
        "POST",
        "/authorize_interaction",
        302
    ));
}

#[test]
fn outbox_kick_skips_reads_failures_and_the_drain_endpoint() {
    assert!(!request_may_enqueue_outbox_work(
        "GET",
        "/api/v1/timelines/home",
        200
    ));
    assert!(!request_may_enqueue_outbox_work(
        "HEAD",
        "/users/alice",
        200
    ));
    assert!(!request_may_enqueue_outbox_work(
        "POST",
        "/api/v1/accounts/account-1/follow",
        401
    ));
    assert!(!request_may_enqueue_outbox_work(
        "POST",
        "/api/v1/accounts/account-1/follow",
        500
    ));
    assert!(!request_may_enqueue_outbox_work(
        "POST",
        "/internal/outbox/process",
        202
    ));
}

#[test]
fn outbox_continuation_requires_batch_progress() {
    assert!(!outbox_batch_made_progress(
        &OutboxProcessResponse::default()
    ));
    assert!(outbox_batch_made_progress(&OutboxProcessResponse {
        delivered: 1,
        ..OutboxProcessResponse::default()
    }));
    assert!(outbox_batch_made_progress(&OutboxProcessResponse {
        expanded: 1,
        ..OutboxProcessResponse::default()
    }));
    assert!(outbox_batch_made_progress(&OutboxProcessResponse {
        failed: 1,
        ..OutboxProcessResponse::default()
    }));
    assert!(outbox_batch_made_progress(&OutboxProcessResponse {
        completed_without_targets: 1,
        ..OutboxProcessResponse::default()
    }));
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
