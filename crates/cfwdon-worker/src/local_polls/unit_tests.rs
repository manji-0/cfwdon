use crate::local_polls::normalize_status_poll;
use crate::statuses::CreateStatusPollRequest;
use cfwdon_core::AppConfig;

#[test]
fn normalize_status_poll_accepts_minimal_valid_poll() {
    let config = AppConfig::default();
    let poll = normalize_status_poll(
        Some(CreateStatusPollRequest {
            options: Some(vec![" One ".to_owned(), "Two".to_owned(), String::new()]),
            expires_in: Some(600),
            multiple: Some(true),
            hide_totals: Some(true),
        }),
        &config,
    )
    .unwrap()
    .unwrap();

    assert_eq!(poll.options(), &["One".to_owned(), "Two".to_owned()]);
    assert_eq!(poll.expires_in_seconds(), 600);
    assert!(poll.multiple());
    assert!(poll.hide_totals());
}

#[test]
fn normalize_status_poll_ignores_empty_form_scaffold() {
    let config = AppConfig::default();
    assert!(
        normalize_status_poll(
            Some(CreateStatusPollRequest {
                options: Some(vec![String::new(), "  ".to_owned()]),
                expires_in: None,
                multiple: None,
                hide_totals: None,
            }),
            &config,
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn normalize_status_poll_rejects_invalid_shapes() {
    let config = AppConfig::default();
    assert!(
        normalize_status_poll(
            Some(CreateStatusPollRequest {
                options: Some(vec!["Only one".to_owned()]),
                expires_in: Some(600),
                multiple: None,
                hide_totals: None,
            }),
            &config,
        )
        .is_err()
    );
    assert!(
        normalize_status_poll(
            Some(CreateStatusPollRequest {
                options: Some(vec!["One".to_owned(), "Two".to_owned()]),
                expires_in: Some(60),
                multiple: None,
                hide_totals: None,
            }),
            &config,
        )
        .is_err()
    );
}
