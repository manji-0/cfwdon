use serde::{Deserialize, Serialize};
use worker::{
    DurableObject, Env, Method, Request, RequestInit, Response, Result, State, WebSocket,
    WebSocketIncomingMessage, WebSocketPair, console_error, durable_object,
};

const STREAM_HUB_WEBSOCKET_PATH: &str = "/websocket";
const STREAM_HUB_PUBLISH_PATH: &str = "/publish";
const STREAM_HUB_INTERNAL_ORIGIN: &str = "https://stream-hub";

/// Publish body accepted by the hub. Extra fields (`account_id`, `event_id`) are
/// carried by the Worker for logging and ignored here.
#[derive(Debug, Deserialize)]
struct StreamHubPublishRequest {
    stream: String,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    list: Option<String>,
    event: String,
    payload: String,
}

#[derive(Debug, Deserialize)]
struct StreamHubWebSocketClientMessage {
    #[serde(rename = "type")]
    message_type: String,
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
}

/// One Mastodon subscription key. A single socket may hold several of them, so a
/// hub that serves more than one channel still routes events correctly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StreamSubscription {
    stream: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    list: Option<String>,
}

impl StreamSubscription {
    fn new(stream: String, tag: Option<String>, list: Option<String>) -> Self {
        Self {
            stream,
            tag: normalized_subscription_value(tag),
            list: normalized_subscription_value(list),
        }
    }

    fn matches(&self, stream: &str, tag: Option<&str>, list: Option<&str>) -> bool {
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

fn normalized_subscription_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SocketSubscriptionState {
    #[serde(default)]
    subscriptions: Vec<StreamSubscription>,
}

impl SocketSubscriptionState {
    fn matches(&self, stream: &str, tag: Option<&str>, list: Option<&str>) -> bool {
        self.subscriptions
            .iter()
            .any(|subscription| subscription.matches(stream, tag, list))
    }
}

#[durable_object]
pub struct StreamHub {
    state: State,
    #[allow(dead_code)]
    env: Env,
}

impl DurableObject for StreamHub {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let path = req.path();
        if req.method() == Method::Post && path.ends_with(STREAM_HUB_PUBLISH_PATH) {
            return self.handle_publish(req).await;
        }

        if path.ends_with(STREAM_HUB_WEBSOCKET_PATH) && websocket_upgrade_requested(&req)? {
            return self.handle_websocket_upgrade(req).await;
        }

        Response::error("Not found", 404)
    }

    async fn websocket_message(
        &self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        let text = match message {
            WebSocketIncomingMessage::String(text) => text,
            WebSocketIncomingMessage::Binary(_) => {
                ws.send_with_str(stream_hub_websocket_error_message(
                    "Only text websocket messages are supported",
                    400,
                ))?;
                return Ok(());
            }
        };

        let client_message = match serde_json::from_str::<StreamHubWebSocketClientMessage>(&text) {
            Ok(message) => message,
            Err(error) => {
                ws.send_with_str(stream_hub_websocket_error_message(
                    &format!("Malformed streaming message: {error}"),
                    400,
                ))?;
                return Ok(());
            }
        };

        if !matches!(
            client_message.message_type.as_str(),
            "subscribe" | "unsubscribe"
        ) {
            ws.send_with_str(stream_hub_websocket_error_message(
                "Unknown streaming message type",
                400,
            ))?;
            return Ok(());
        }

        let stream_name = match client_message.stream.as_deref() {
            Some(stream) if !stream.trim().is_empty() => stream.to_owned(),
            _ => {
                ws.send_with_str(stream_hub_websocket_error_message(
                    "Unknown stream type",
                    400,
                ))?;
                return Ok(());
            }
        };

        let mut state = ws
            .deserialize_attachment::<SocketSubscriptionState>()?
            .unwrap_or_default();
        let subscription =
            StreamSubscription::new(stream_name, client_message.tag, client_message.list);

        if client_message.message_type == "subscribe" {
            if !state.subscriptions.contains(&subscription) {
                state.subscriptions.push(subscription);
            }
        } else {
            state
                .subscriptions
                .retain(|existing| existing != &subscription);
        }

        ws.serialize_attachment(&state)?;
        Ok(())
    }

    async fn websocket_close(
        &self,
        _ws: WebSocket,
        _code: usize,
        _reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        Ok(())
    }
}

impl StreamHub {
    async fn handle_publish(&self, mut req: Request) -> Result<Response> {
        let publish = req.json::<StreamHubPublishRequest>().await?;
        let fanout_message = stream_hub_fanout_message(
            &publish.stream,
            publish.tag.as_deref(),
            publish.list.as_deref(),
            &publish.event,
            &publish.payload,
        )?;

        let mut delivered = 0usize;
        for socket in self.state.get_websockets() {
            let state = socket
                .deserialize_attachment::<SocketSubscriptionState>()?
                .unwrap_or_default();
            if !state.matches(
                &publish.stream,
                publish.tag.as_deref(),
                publish.list.as_deref(),
            ) {
                continue;
            }

            if socket.send_with_str(&fanout_message).is_ok() {
                delivered += 1;
            }
        }

        Response::from_json(&serde_json::json!({ "delivered": delivered }))
    }

    async fn handle_websocket_upgrade(&self, req: Request) -> Result<Response> {
        let params = stream_hub_connect_params(&req)?;
        let pair = WebSocketPair::new()?;

        let mut tags = Vec::new();
        if let Some(stream) = params.stream.as_deref() {
            tags.push(stream_subscription_tag(stream));
            if let Some(account_id) = params.account_id.as_deref() {
                tags.push(account_subscription_tag(account_id));
            }
            if let Some(tag) = params.tag.as_deref() {
                tags.push(tag_subscription_tag(tag));
            }
            if let Some(list) = params.list.as_deref() {
                tags.push(list_subscription_tag(list));
            }
        }

        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        if tag_refs.is_empty() {
            self.state.accept_web_socket(&pair.server);
        } else {
            self.state
                .accept_websocket_with_tags(&pair.server, &tag_refs);
        }

        let mut state = SocketSubscriptionState::default();
        if let Some(stream) = params.stream {
            state
                .subscriptions
                .push(StreamSubscription::new(stream, params.tag, params.list));
        }
        pair.server.serialize_attachment(&state)?;

        let mut response = Response::from_websocket(pair.client)?;
        if let Some(protocol) = req
            .headers()
            .get("Sec-WebSocket-Protocol")?
            .filter(|value| !value.trim().is_empty())
        {
            response
                .headers_mut()
                .set("Sec-WebSocket-Protocol", &protocol)?;
        }
        Ok(response)
    }
}

#[derive(Debug, Default, Deserialize)]
struct StreamHubConnectParams {
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    account_id: Option<String>,
}

fn stream_hub_connect_params(req: &Request) -> Result<StreamHubConnectParams> {
    let mut params = req.query::<StreamHubConnectParams>().unwrap_or_default();

    if params.account_id.is_none() {
        params.account_id =
            header_value(req, "X-Account-Id").or_else(|| header_value(req, "Cf-Account-Id"));
    }
    if params.stream.is_none() {
        params.stream = header_value(req, "X-Stream");
    }
    if params.tag.is_none() {
        params.tag = header_value(req, "X-Stream-Tag");
    }
    if params.list.is_none() {
        params.list = header_value(req, "X-Stream-List");
    }

    Ok(params)
}

fn header_value(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .get(name)
        .ok()
        .flatten()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn websocket_upgrade_requested(req: &Request) -> Result<bool> {
    Ok(req
        .headers()
        .get("Upgrade")?
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket")))
}

/// Optional shard suffix for hot public hubs. When `shard_count <= 1`, returns `base_hub` unchanged.
pub(crate) fn stream_hub_sharded_id_name(
    base_hub: &str,
    shard_key: &str,
    shard_count: u32,
) -> String {
    if shard_count <= 1 {
        return base_hub.to_owned();
    }
    let index = (stream_hub_shard_hash(shard_key) % u64::from(shard_count)) as u32;
    format!("{base_hub}#{index}")
}

fn stream_hub_shard_hash(shard_key: &str) -> u64 {
    let mut hash = 14695981039346656037_u64;
    for byte in shard_key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

/// Every authenticated channel (`user`, `user:notification`, `direct`, `list`)
/// is served by one hub per account. Subscribers of those channels always
/// resolve to a single account, so a per-account session hub lets one socket
/// hold several subscriptions and keeps clients off other accounts' hubs.
pub(crate) fn stream_hub_session_id_name(account_id: &str) -> String {
    format!("user:{account_id}")
}

/// Shared hub for a channel with an open audience.
pub(crate) fn stream_hub_channel_id_name(stream: &str, tag: Option<&str>) -> String {
    if stream.starts_with("hashtag") {
        return format!("hashtag:{}", tag.unwrap_or_default());
    }
    stream.to_owned()
}

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

pub(crate) async fn publish_stream_hub_event(
    env: &Env,
    binding: &str,
    hub_name: &str,
    body: &serde_json::Value,
) -> Result<()> {
    let namespace = env.durable_object(binding)?;
    let stub = namespace.get_by_name(hub_name)?;
    let body_json = serde_json::to_string(body).map_err(|error| {
        worker::Error::RustError(format!("failed to encode stream hub publish body: {error}"))
    })?;

    let headers = worker::Headers::new();
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body_json)));

    let url = format!("{STREAM_HUB_INTERNAL_ORIGIN}{STREAM_HUB_PUBLISH_PATH}");
    let request = Request::new_with_init(&url, &init)?;
    let response = stub.fetch_with_request(request).await?;
    if response.status_code() >= 400 {
        return Err(worker::Error::RustError(format!(
            "stream hub publish failed with HTTP {}",
            response.status_code()
        )));
    }
    Ok(())
}

pub(crate) async fn connect_stream_hub_websocket(
    env: &Env,
    binding: &str,
    hub_name: &str,
    stream: &str,
    tag: Option<&str>,
    list: Option<&str>,
    account_id: Option<&str>,
) -> Result<WebSocket> {
    let namespace = env.durable_object(binding)?;
    let stub = namespace.get_by_name(hub_name)?;

    let headers = worker::Headers::new();
    headers.set("Upgrade", "websocket")?;
    headers.set("X-Stream", stream)?;
    if let Some(tag_value) = tag.map(str::trim).filter(|value| !value.is_empty()) {
        headers.set("X-Stream-Tag", tag_value)?;
    }
    if let Some(list_value) = list.map(str::trim).filter(|value| !value.is_empty()) {
        headers.set("X-Stream-List", list_value)?;
    }
    if let Some(account_id) = account_id.map(str::trim).filter(|value| !value.is_empty()) {
        headers.set("X-Account-Id", account_id)?;
    }

    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let url = format!("{STREAM_HUB_INTERNAL_ORIGIN}{STREAM_HUB_WEBSOCKET_PATH}");
    let request = Request::new_with_init(&url, &init)?;
    let response = stub.fetch_with_request(request).await?;
    let websocket = response.websocket().ok_or_else(|| {
        worker::Error::RustError("stream hub websocket upgrade did not return a socket".to_owned())
    })?;
    websocket.accept()?;
    Ok(websocket)
}

pub(crate) async fn upgrade_stream_hub_websocket(
    env: &Env,
    binding: &str,
    hub_name: &str,
    req: Request,
) -> Result<Response> {
    let namespace = env.durable_object(binding)?;
    let stub = namespace.get_by_name(hub_name)?;

    let query = req
        .url()?
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let url = format!("{STREAM_HUB_INTERNAL_ORIGIN}{STREAM_HUB_WEBSOCKET_PATH}{query}");

    let headers = worker::Headers::new();
    for (name, value) in req.headers().entries() {
        headers.set(&name, &value)?;
    }
    if !headers
        .get("Upgrade")?
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
    {
        headers.set("Upgrade", "websocket")?;
    }

    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let request = Request::new_with_init(&url, &init)?;
    stub.fetch_with_request(request).await
}

fn stream_subscription_tag(stream: &str) -> String {
    format!("stream:{stream}")
}

fn account_subscription_tag(account_id: &str) -> String {
    format!("account:{account_id}")
}

fn tag_subscription_tag(tag: &str) -> String {
    format!("tag:{tag}")
}

fn list_subscription_tag(list: &str) -> String {
    format!("list:{list}")
}

fn stream_hub_stream_labels(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) -> Vec<String> {
    let mut labels = vec![stream_name.to_owned()];
    if stream_name.starts_with("hashtag")
        && let Some(tag) = tag
    {
        labels.push(tag.to_owned());
    }
    if stream_name == "list"
        && let Some(list) = list
    {
        labels.push(list.to_owned());
    }
    labels
}

pub(crate) fn stream_hub_fanout_message(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
    event: &str,
    payload: &str,
) -> Result<String> {
    let mut message = serde_json::json!({
        "stream": stream_hub_stream_labels(stream_name, tag, list),
        "event": event,
    });
    if event != "filters_changed" {
        message["payload"] = serde_json::Value::String(payload.to_owned());
    }
    serde_json::to_string(&message).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize stream hub websocket event: {error}"
        ))
    })
}

fn stream_hub_websocket_error_message(message: &str, status: u16) -> String {
    serde_json::json!({
        "error": message,
        "status": status,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_hub_sharded_id_name_returns_base_hub_when_unsharded() {
        assert_eq!(
            stream_hub_sharded_id_name("public", "status-1", 1),
            "public"
        );
        assert_eq!(
            stream_hub_sharded_id_name("public:local", "status-1", 0),
            "public:local"
        );
    }

    #[test]
    fn stream_hub_sharded_id_name_is_stable_and_within_shard_range() {
        let first = stream_hub_sharded_id_name("public", "status-abc", 4);
        let second = stream_hub_sharded_id_name("public", "status-abc", 4);
        assert_eq!(first, second);
        assert!(first.starts_with("public#"));
        let index = first
            .strip_prefix("public#")
            .and_then(|suffix| suffix.parse::<u32>().ok())
            .expect("shard suffix");
        assert!(index < 4);

        assert_ne!(
            stream_hub_sharded_id_name("public", "status-abc", 4),
            stream_hub_sharded_id_name("public", "status-xyz", 4)
        );
    }

    #[test]
    fn session_channels_share_one_hub_per_account() {
        assert_eq!(stream_hub_session_id_name("acct-1"), "user:acct-1");
        assert_ne!(
            stream_hub_session_id_name("acct-1"),
            stream_hub_session_id_name("acct-2")
        );
    }

    #[test]
    fn stream_hub_channel_id_name_maps_open_channels() {
        assert_eq!(
            stream_hub_channel_id_name("hashtag", Some("rust")),
            "hashtag:rust"
        );
        assert_eq!(
            stream_hub_channel_id_name("hashtag:local", Some("rust")),
            "hashtag:rust"
        );
        assert_eq!(
            stream_hub_channel_id_name("public:local", None),
            "public:local"
        );
    }

    #[test]
    fn stream_hub_fanout_message_matches_mastodon_shape() {
        let message =
            stream_hub_fanout_message("user:notification", None, None, "update", r#"{"id":"1"}"#)
                .unwrap();
        let parsed = serde_json::from_str::<serde_json::Value>(&message).unwrap();
        assert_eq!(parsed["stream"], serde_json::json!(["user:notification"]));
        assert_eq!(parsed["event"], "update");
        assert_eq!(parsed["payload"], r#"{"id":"1"}"#);
    }

    #[test]
    fn stream_hub_fanout_message_omits_payload_for_filters_changed() {
        let message =
            stream_hub_fanout_message("user", None, None, "filters_changed", "{}").unwrap();
        let parsed = serde_json::from_str::<serde_json::Value>(&message).unwrap();
        assert_eq!(parsed["stream"], serde_json::json!(["user"]));
        assert_eq!(parsed["event"], "filters_changed");
        assert!(parsed.get("payload").is_none());
    }

    #[test]
    fn stream_hub_fanout_message_includes_hashtag_and_list_labels() {
        let hashtag_message =
            stream_hub_fanout_message("hashtag:local", Some("rust"), None, "update", "{}").unwrap();
        let hashtag = serde_json::from_str::<serde_json::Value>(&hashtag_message).unwrap();
        assert_eq!(
            hashtag["stream"],
            serde_json::json!(["hashtag:local", "rust"])
        );

        let list_message =
            stream_hub_fanout_message("list", None, Some("list-1"), "update", "{}").unwrap();
        let list = serde_json::from_str::<serde_json::Value>(&list_message).unwrap();
        assert_eq!(list["stream"], serde_json::json!(["list", "list-1"]));
    }

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
            ],
        };

        assert!(state.matches("user", None, None));
        assert!(state.matches("user:notification", None, None));
        assert!(state.matches("list", None, Some("list-1")));
        assert!(!state.matches("list", None, Some("list-9")));
        assert!(!state.matches("direct", None, None));
    }
}
