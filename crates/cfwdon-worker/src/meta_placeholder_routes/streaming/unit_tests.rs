use crate::meta_placeholder_routes::streaming::{
    StreamingChannelValidationError, streaming_channel_requires_auth,
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
        validate_streaming_channel_request(Some("public"), None, None, Some("health")).unwrap_err(),
        StreamingChannelValidationError::UnknownChannelRequested
    );
}

#[test]
fn streaming_channel_requires_auth_matches_user_scoped_streams() {
    assert!(streaming_channel_requires_auth("user"));
    assert!(streaming_channel_requires_auth("user:notification"));
    assert!(streaming_channel_requires_auth("list"));
    assert!(streaming_channel_requires_auth("direct"));
    assert!(!streaming_channel_requires_auth("public"));
    assert!(!streaming_channel_requires_auth("hashtag"));
}
