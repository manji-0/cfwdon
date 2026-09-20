use serde::{Deserialize, Serialize};

/// Publish body accepted by the hub. Extra fields (`account_id`, `event_id`) are
/// carried by the Worker for logging and ignored here.
#[derive(Debug, Deserialize)]
pub(super) struct StreamHubPublishRequest {
    pub(super) stream: String,
    #[serde(default)]
    pub(super) tag: Option<String>,
    #[serde(default)]
    pub(super) list: Option<String>,
    pub(super) event: String,
    pub(super) payload: String,
}

impl StreamHubPublishRequest {
    /// Body used when relaying to a session hub.
    pub(super) fn to_body(&self) -> serde_json::Value {
        let mut body = serde_json::json!({
            "stream": self.stream,
            "event": self.event,
            "payload": self.payload,
        });
        if let Some(tag) = self.tag.as_deref() {
            body["tag"] = serde_json::json!(tag);
        }
        if let Some(list) = self.list.as_deref() {
            body["list"] = serde_json::json!(list);
        }
        body
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamHubWebSocketClientMessage {
    #[serde(rename = "type")]
    pub(super) message_type: String,
    pub(super) stream: Option<String>,
    pub(super) tag: Option<String>,
    pub(super) list: Option<String>,
}

/// One Mastodon subscription key. A single socket may hold several of them, so a
/// hub that serves more than one channel still routes events correctly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StreamSubscription {
    pub(super) stream: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) list: Option<String>,
}

impl StreamSubscription {
    pub(super) fn new(stream: String, tag: Option<String>, list: Option<String>) -> Self {
        Self {
            stream,
            tag: normalized_subscription_value(tag),
            list: normalized_subscription_value(list),
        }
    }

    pub(super) fn matches(&self, stream: &str, tag: Option<&str>, list: Option<&str>) -> bool {
        if self.stream != stream {
            return false;
        }
        if self.stream.starts_with("hashtag") {
            return self.tag.as_deref() == tag;
        }
        if self.stream == "list" {
            return self.list.as_deref() == list;
        }
        true
    }
}

pub(super) fn normalized_subscription_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct SocketSubscriptionState {
    #[serde(default)]
    pub(super) subscriptions: Vec<StreamSubscription>,
    /// Session hub of the account behind this socket, when authenticated. Open
    /// channels forward their events here so one socket can mix `user` and
    /// `public` subscriptions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) session_hub: Option<String>,
}

impl SocketSubscriptionState {
    pub(super) fn matches(&self, stream: &str, tag: Option<&str>, list: Option<&str>) -> bool {
        self.subscriptions
            .iter()
            .any(|subscription| subscription.matches(stream, tag, list))
    }
}

/// A session hub that asked to receive this open channel's events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ForwardTarget {
    pub(super) hub: String,
    pub(super) subscription: StreamSubscription,
}

#[derive(Debug, Deserialize)]
pub(super) struct ForwardRegisterRequest {
    pub(super) hub: String,
    pub(super) stream: String,
    #[serde(default)]
    pub(super) tag: Option<String>,
    #[serde(default)]
    pub(super) list: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::super::naming::stream_hub_session_id_name;
    use super::*;

    #[test]
    fn subscription_matches_requires_same_stream() {
        let subscription = StreamSubscription::new("user".to_owned(), None, None);
        assert!(subscription.matches("user", None, None));
        assert!(!subscription.matches("user:notification", None, None));
        assert!(!subscription.matches("direct", None, None));
    }

    #[test]
    fn subscription_matches_compares_tag_and_list_keys() {
        let hashtag =
            StreamSubscription::new("hashtag:local".to_owned(), Some(" rust ".to_owned()), None);
        assert!(hashtag.matches("hashtag:local", Some("rust"), None));
        assert!(!hashtag.matches("hashtag:local", Some("wasm"), None));
        assert!(!hashtag.matches("hashtag:local", None, None));
        assert!(!hashtag.matches("hashtag", Some("rust"), None));

        let list = StreamSubscription::new("list".to_owned(), None, Some("list-1".to_owned()));
        assert!(list.matches("list", None, Some("list-1")));
        assert!(!list.matches("list", None, Some("list-2")));
        assert!(!list.matches("list", None, None));
    }

    #[test]
    fn socket_without_subscriptions_receives_nothing() {
        let state = SocketSubscriptionState::default();
        assert!(!state.matches("user", None, None));
        assert!(!state.matches("direct", None, None));
    }

    #[test]
    fn socket_can_hold_several_subscriptions_on_one_hub() {
        let state = SocketSubscriptionState {
            subscriptions: vec![
                StreamSubscription::new("user".to_owned(), None, None),
                StreamSubscription::new("user:notification".to_owned(), None, None),
                StreamSubscription::new("list".to_owned(), None, Some("list-1".to_owned())),
                StreamSubscription::new("public:local".to_owned(), None, None),
            ],
            session_hub: Some(stream_hub_session_id_name("acct-1")),
        };

        assert!(state.matches("user", None, None));
        assert!(state.matches("user:notification", None, None));
        assert!(state.matches("list", None, Some("list-1")));
        assert!(state.matches("public:local", None, None));
        assert!(!state.matches("list", None, Some("list-9")));
        assert!(!state.matches("direct", None, None));
        assert!(!state.matches("public", None, None));
    }

    #[test]
    fn forward_target_matches_only_its_own_subscription_key() {
        let target = ForwardTarget {
            hub: stream_hub_session_id_name("acct-1"),
            subscription: StreamSubscription::new(
                "hashtag:local".to_owned(),
                Some("rust".to_owned()),
                None,
            ),
        };

        assert!(
            target
                .subscription
                .matches("hashtag:local", Some("rust"), None)
        );
        assert!(!target.subscription.matches("hashtag", Some("rust"), None));
        assert!(
            !target
                .subscription
                .matches("hashtag:local", Some("wasm"), None)
        );
    }
}
