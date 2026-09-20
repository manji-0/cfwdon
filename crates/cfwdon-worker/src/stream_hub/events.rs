use super::client::publish_stream_hub_event;
use super::naming::stream_hub_session_id_name;
use worker::{Env, console_error};

/// One streaming event addressed to a single hub. `tag` / `list` carry the
/// Mastodon subscription key so the hub can route events for the channels it
/// serves without depending on per-socket state.
#[derive(Debug, Clone)]
pub(crate) struct StreamHubEvent<'a> {
    pub(crate) stream: &'a str,
    pub(crate) tag: Option<&'a str>,
    pub(crate) list: Option<&'a str>,
    pub(crate) account_id: Option<&'a str>,
    pub(crate) event: &'a str,
    pub(crate) payload: &'a str,
    pub(crate) event_id: Option<&'a str>,
}

impl<'a> StreamHubEvent<'a> {
    pub(crate) fn new(
        stream: &'a str,
        event: &'a str,
        payload: &'a str,
        event_id: Option<&'a str>,
    ) -> Self {
        Self {
            stream,
            tag: None,
            list: None,
            account_id: None,
            event,
            payload,
            event_id,
        }
    }

    pub(crate) fn with_tag(mut self, tag: Option<&'a str>) -> Self {
        self.tag = tag;
        self
    }

    pub(crate) fn with_list(mut self, list: Option<&'a str>) -> Self {
        self.list = list;
        self
    }

    pub(crate) fn with_account_id(mut self, account_id: Option<&'a str>) -> Self {
        self.account_id = account_id;
        self
    }

    fn to_body(&self) -> serde_json::Value {
        let mut body = serde_json::json!({
            "stream": self.stream,
            "event": self.event,
            "payload": self.payload,
        });
        if let Some(tag) = self.tag {
            body["tag"] = serde_json::json!(tag);
        }
        if let Some(list) = self.list {
            body["list"] = serde_json::json!(list);
        }
        if let Some(account_id) = self.account_id {
            body["account_id"] = serde_json::json!(account_id);
        }
        if let Some(event_id) = self.event_id {
            body["event_id"] = serde_json::json!(event_id);
        }
        body
    }
}

pub(crate) async fn publish_stream_hub_event_soft(
    env: &Env,
    binding: &str,
    hub_name: &str,
    event: &StreamHubEvent<'_>,
) {
    if let Err(error) = publish_stream_hub_event(env, binding, hub_name, &event.to_body()).await {
        console_error!(
            "failed to publish stream hub event ({}) to hub {hub_name} stream {}: {error}",
            event.event,
            event.stream
        );
    }
}

pub(crate) async fn publish_user_stream_hub_event_soft(
    env: &Env,
    binding: &str,
    account_id: &str,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    let hub_name = stream_hub_session_id_name(account_id);
    publish_stream_hub_event_soft(
        env,
        binding,
        &hub_name,
        &StreamHubEvent::new("user", event, payload, event_id).with_account_id(Some(account_id)),
    )
    .await;
}

/// Publish a `direct` channel event to the recipient's session hub.
pub(crate) async fn publish_direct_stream_hub_event_soft(
    env: &Env,
    binding: &str,
    account_id: &str,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    let hub_name = stream_hub_session_id_name(account_id);
    publish_stream_hub_event_soft(
        env,
        binding,
        &hub_name,
        &StreamHubEvent::new("direct", event, payload, event_id).with_account_id(Some(account_id)),
    )
    .await;
}

/// Publish a `list` channel event to the list owner's session hub.
pub(crate) async fn publish_list_stream_hub_event_soft(
    env: &Env,
    binding: &str,
    owner_account_id: &str,
    list_id: &str,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    let hub_name = stream_hub_session_id_name(owner_account_id);
    publish_stream_hub_event_soft(
        env,
        binding,
        &hub_name,
        &StreamHubEvent::new("list", event, payload, event_id)
            .with_list(Some(list_id))
            .with_account_id(Some(owner_account_id)),
    )
    .await;
}

pub(crate) async fn publish_notification_stream_hub_event_soft(
    env: &Env,
    binding: &str,
    account_id: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    let hub_name = stream_hub_session_id_name(account_id);
    publish_stream_hub_event_soft(
        env,
        binding,
        &hub_name,
        &StreamHubEvent::new("user:notification", "notification", payload, event_id)
            .with_account_id(Some(account_id)),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::super::messages::StreamHubPublishRequest;
    use super::*;

    #[test]
    fn stream_hub_publish_body_shape() {
        let body = StreamHubEvent::new("user", "update", r#"{"id":"1"}"#, Some("evt-1"))
            .with_account_id(Some("acct-1"))
            .to_body();
        let publish = serde_json::from_value::<StreamHubPublishRequest>(body).unwrap();
        assert_eq!(publish.stream, "user");
        assert_eq!(publish.event, "update");
        assert_eq!(publish.payload, r#"{"id":"1"}"#);
        assert!(publish.tag.is_none());
        assert!(publish.list.is_none());
    }

    #[test]
    fn stream_hub_event_body_carries_subscription_key() {
        let hashtag = StreamHubEvent::new("hashtag:local", "update", "{}", None)
            .with_tag(Some("rust"))
            .to_body();
        assert_eq!(hashtag["tag"], "rust");

        let list = StreamHubEvent::new("list", "update", "{}", None)
            .with_list(Some("list-1"))
            .to_body();
        assert_eq!(list["list"], "list-1");
    }
}
