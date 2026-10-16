use crate::meta_placeholder_routes::streaming::{
    StreamingChannelValidationError, is_streaming_health_path, streaming_channel_requires_auth,
    validate_streaming_channel_request,
};

#[test]
fn validate_streaming_channel_request_accepts_known_streams() {
    assert_eq!(
        validate_streaming_channel_request(Some(" User "), None, None, None).unwrap(),
        "user"
    );
    assert_eq!(
        validate_streaming_channel_request(Some("hashtag:local"), Some("rust"), None, None)
            .unwrap(),
        "hashtag:local"
    );
    assert_eq!(
        validate_streaming_channel_request(Some("list"), None, Some("123"), None).unwrap(),
        "list"
    );
}

#[test]
fn validate_streaming_channel_request_rejects_unknown_or_incomplete_channels() {
    assert_eq!(
        validate_streaming_channel_request(Some("hashtag"), None, None, None).unwrap_err(),
        StreamingChannelValidationError::MissingTag
    );
    assert_eq!(
        validate_streaming_channel_request(Some("list"), None, None, None).unwrap_err(),
        StreamingChannelValidationError::MissingList
    );
    assert_eq!(
        validate_streaming_channel_request(Some("public"), None, None, Some("bogus")).unwrap_err(),
        StreamingChannelValidationError::UnknownChannelRequested
    );
}

#[test]
fn streaming_health_path_is_detected_case_insensitively() {
    assert!(is_streaming_health_path(Some("health")));
    assert!(is_streaming_health_path(Some("Health")));
    assert!(!is_streaming_health_path(Some("user")));
    assert!(!is_streaming_health_path(None));
}

#[test]
fn streaming_channel_requires_auth_for_every_mastodon_channel() {
    for stream in [
        "user",
        "user:notification",
        "list",
        "direct",
        "public",
        "public:media",
        "public:local",
        "public:local:media",
        "public:remote",
        "public:remote:media",
        "hashtag",
        "hashtag:local",
    ] {
        assert!(streaming_channel_requires_auth(stream), "{stream}");
    }
    assert!(!streaming_channel_requires_auth("unknown"));
}
