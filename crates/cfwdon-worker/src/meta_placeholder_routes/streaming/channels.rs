use crate::{Request, Response, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(super) struct StreamingQuery {
    pub(super) stream: Option<String>,
    pub(super) tag: Option<String>,
    pub(super) list: Option<String>,
    pub(super) access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamingWebSocketClientMessage {
    #[serde(rename = "type")]
    pub(super) message_type: String,
    pub(super) stream: Option<String>,
    pub(super) tag: Option<String>,
    pub(super) list: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamingChannelValidationError {
    UnknownChannelRequested,
    MissingTag,
    MissingList,
}

pub(crate) fn normalize_streaming_channel(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

pub(super) fn streaming_channel_requires_tag(stream: &str) -> bool {
    matches!(stream, "hashtag" | "hashtag:local")
}

pub(super) fn streaming_channel_requires_list(stream: &str) -> bool {
    stream == "list"
}

pub(super) fn normalize_streaming_path_channel(value: &str) -> Option<String> {
    let path = value.trim().trim_matches('/');
    if path.is_empty() {
        return None;
    }
    normalize_streaming_channel(Some(&path.replace('/', ":")))
}

pub(crate) fn streaming_channel_requires_auth(stream: &str) -> bool {
    matches!(stream, "user" | "user:notification" | "list" | "direct")
}

pub(crate) fn validate_streaming_channel_request(
    stream: Option<&str>,
    tag: Option<&str>,
    list: Option<&str>,
    extra_path: Option<&str>,
) -> std::result::Result<String, StreamingChannelValidationError> {
    let stream = match extra_path.map(str::trim).filter(|value| !value.is_empty()) {
        Some(_)
            if stream
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some() =>
        {
            return Err(StreamingChannelValidationError::UnknownChannelRequested);
        }
        Some(path) => normalize_streaming_path_channel(path),
        None => normalize_streaming_channel(stream),
    };
    let Some(stream) = stream else {
        return Err(StreamingChannelValidationError::UnknownChannelRequested);
    };
    if !matches!(
        stream.as_str(),
        "public"
            | "public:media"
            | "public:local"
            | "public:local:media"
            | "public:remote"
            | "public:remote:media"
            | "hashtag"
            | "hashtag:local"
            | "user"
            | "user:notification"
            | "list"
            | "direct"
    ) {
        return Err(StreamingChannelValidationError::UnknownChannelRequested);
    }
    if streaming_channel_requires_tag(&stream)
        && tag
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(StreamingChannelValidationError::MissingTag);
    }
    if streaming_channel_requires_list(&stream)
        && list
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(StreamingChannelValidationError::MissingList);
    }
    Ok(stream)
}

pub(super) fn streaming_bad_request_response(
    error: StreamingChannelValidationError,
) -> Result<Response> {
    let message = match error {
        StreamingChannelValidationError::UnknownChannelRequested => "Unknown channel requested",
        StreamingChannelValidationError::MissingTag => "Missing tag parameter",
        StreamingChannelValidationError::MissingList => "Missing list parameter",
    };
    Ok(Response::from_json(&serde_json::json!({
        "error": message,
    }))?
    .with_status(400))
}

pub(super) fn websocket_protocol_access_token(req: &Request) -> Result<Option<String>> {
    let Some(protocols) = req.headers().get("Sec-WebSocket-Protocol")? else {
        return Ok(None);
    };

    Ok(protocols
        .split(',')
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned))
}

pub(super) fn streaming_channel_supports_live_events(stream: &str) -> bool {
    matches!(
        stream,
        "public"
            | "public:media"
            | "public:local"
            | "public:local:media"
            | "public:remote"
            | "public:remote:media"
            | "hashtag"
            | "hashtag:local"
            | "user"
            | "user:notification"
            | "list"
            | "direct"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_channel_validation_accepts_query_and_path_channels() {
        assert_eq!(
            validate_streaming_channel_request(Some("public"), None, None, None).unwrap(),
            "public"
        );
        assert_eq!(
            validate_streaming_channel_request(None, None, None, Some("user")).unwrap(),
            "user"
        );
        assert_eq!(
            validate_streaming_channel_request(None, None, None, Some("public/local/media"))
                .unwrap(),
            "public:local:media"
        );
        assert_eq!(
            validate_streaming_channel_request(None, Some("rust"), None, Some("hashtag")).unwrap(),
            "hashtag"
        );
    }

    #[test]
    fn streaming_channel_validation_rejects_conflicting_query_and_path_channels() {
        assert!(matches!(
            validate_streaming_channel_request(Some("public"), None, None, Some("user")),
            Err(StreamingChannelValidationError::UnknownChannelRequested)
        ));
    }

    #[test]
    fn streaming_channel_supports_live_events_for_validated_channels() {
        assert!(streaming_channel_supports_live_events("public"));
        assert!(streaming_channel_supports_live_events("user:notification"));
        assert!(streaming_channel_supports_live_events("direct"));
        assert!(!streaming_channel_supports_live_events("unknown"));
    }
}
