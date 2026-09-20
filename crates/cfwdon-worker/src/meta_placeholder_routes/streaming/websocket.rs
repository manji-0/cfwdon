use super::STREAMING_POLL_INTERVAL_SECS;
use super::budget::{streaming_error_is_subrequest_limit, streaming_poll_budget_exhausted};
use super::channels::{
    StreamingChannelValidationError, StreamingWebSocketClientMessage,
    streaming_channel_requires_auth, validate_streaming_channel_request,
};
use super::hub_routing::stream_hub_proxy_target;
use super::poll::poll_streaming_events;
use crate::{
    D1Database, Request, Response, Result, StreamHubUpgradeParams, StreamingEvent,
    StreamingLoopState, snapshot_d1_request_metrics, stream_hub_session_id_name,
    upgrade_stream_hub_websocket,
};
use futures_util::{FutureExt, StreamExt, pin_mut, select};
use std::collections::HashMap;
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use worker::{
    Env, WebSocket, WebSocketPair, console_error, console_log, ws_events::WebsocketEvent,
};

pub(super) struct StreamingWebSocketSubscription {
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
    state: StreamingLoopState,
}

impl StreamingWebSocketSubscription {
    fn new(stream_name: String, tag: Option<String>, list: Option<String>) -> Self {
        Self {
            stream_name,
            tag,
            list,
            state: StreamingLoopState::new(),
        }
    }
}

pub(super) fn streaming_websocket_subscription_key(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        stream_name,
        tag.unwrap_or_default(),
        list.unwrap_or_default()
    )
}

pub(super) fn streaming_websocket_stream_labels(
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

pub(super) fn streaming_websocket_event_message(
    subscription: &StreamingWebSocketSubscription,
    event: &StreamingEvent,
) -> Result<String> {
    let mut payload = serde_json::json!({
        "stream": streaming_websocket_stream_labels(
            &subscription.stream_name,
            subscription.tag.as_deref(),
            subscription.list.as_deref(),
        ),
        "event": event.event,
    });
    if event.event != "filters_changed" {
        payload["payload"] = serde_json::Value::String(event.data.clone());
    }
    serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize websocket stream event: {error}"
        ))
    })
}

pub(super) fn streaming_websocket_error_message(message: &str, status: u16) -> String {
    serde_json::json!({
        "error": message,
        "status": status,
    })
    .to_string()
}

pub(super) fn add_streaming_websocket_subscription(
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
) {
    let key = streaming_websocket_subscription_key(&stream_name, tag.as_deref(), list.as_deref());
    subscriptions
        .entry(key)
        .or_insert_with(|| StreamingWebSocketSubscription::new(stream_name, tag, list));
}

pub(super) fn remove_streaming_websocket_subscription(
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) {
    let key = streaming_websocket_subscription_key(stream_name, tag, list);
    subscriptions.remove(&key);
}

pub(super) fn handle_streaming_websocket_client_message(
    websocket: &WebSocket,
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    text: &str,
    viewer: Option<&crate::LocalAccount>,
) -> bool {
    let message = match serde_json::from_str::<StreamingWebSocketClientMessage>(text) {
        Ok(message) => message,
        Err(error) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                &format!("Malformed streaming message: {error}"),
                400,
            ));
            return true;
        }
    };
    if !matches!(message.message_type.as_str(), "subscribe" | "unsubscribe") {
        let _ = websocket.send_with_str(streaming_websocket_error_message(
            "Unknown streaming message type",
            400,
        ));
        return true;
    }
    let stream_name = match validate_streaming_channel_request(
        message.stream.as_deref(),
        message.tag.as_deref(),
        message.list.as_deref(),
        None,
    ) {
        Ok(stream_name) => stream_name,
        Err(StreamingChannelValidationError::UnknownChannelRequested) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Unknown stream type",
                400,
            ));
            return true;
        }
        Err(StreamingChannelValidationError::MissingTag) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Missing tag parameter",
                400,
            ));
            return true;
        }
        Err(StreamingChannelValidationError::MissingList) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Missing list parameter",
                400,
            ));
            return true;
        }
    };
    if streaming_channel_requires_auth(&stream_name) && viewer.is_none() {
        let _ = websocket.send_with_str(streaming_websocket_error_message(
            "The access token is invalid",
            401,
        ));
        return true;
    }
    if message.message_type == "subscribe" {
        add_streaming_websocket_subscription(subscriptions, stream_name, message.tag, message.list);
    } else {
        remove_streaming_websocket_subscription(
            subscriptions,
            &stream_name,
            message.tag.as_deref(),
            message.list.as_deref(),
        );
    }
    true
}

pub(super) async fn poll_streaming_websocket_subscriptions(
    websocket: &WebSocket,
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
) -> bool {
    for subscription in subscriptions.values_mut() {
        if streaming_poll_budget_exhausted(0, 0) {
            return false;
        }
        let events = match poll_streaming_events(
            db,
            config,
            &subscription.stream_name,
            subscription.tag.as_deref(),
            subscription.list.as_deref(),
            viewer,
            &mut subscription.state,
        )
        .await
        {
            Ok(events) => events,
            Err(error) => {
                console_error!(
                    "websocket streaming poll failed stream={} tag={} list={} error={}",
                    subscription.stream_name,
                    subscription.tag.as_deref().unwrap_or(""),
                    subscription.list.as_deref().unwrap_or(""),
                    error
                );
                if streaming_error_is_subrequest_limit(&error) {
                    return false;
                }
                continue;
            }
        };
        for event in events {
            let message = match streaming_websocket_event_message(subscription, &event) {
                Ok(message) => message,
                Err(error) => {
                    console_error!(
                        "websocket streaming event serialization failed stream={} error={}",
                        subscription.stream_name,
                        error
                    );
                    continue;
                }
            };
            if websocket.send_with_str(message).is_err() {
                return false;
            }
        }
    }
    true
}

pub(super) async fn run_streaming_websocket(
    websocket: WebSocket,
    db: D1Database,
    config: cfwdon_core::AppConfig,
    initial_stream: Option<String>,
    initial_tag: Option<String>,
    initial_list: Option<String>,
    viewer: Option<crate::LocalAccount>,
) {
    let mut subscriptions = HashMap::<String, StreamingWebSocketSubscription>::new();
    if let Some(stream_name) = initial_stream {
        add_streaming_websocket_subscription(
            &mut subscriptions,
            stream_name,
            initial_tag,
            initial_list,
        );
    }
    {
        let mut websocket_events = match websocket.events() {
            Ok(events) => events,
            Err(error) => {
                console_error!("failed to attach websocket event stream: {}", error);
                let _ = websocket.close(Some(1011), Some("stream failed"));
                return;
            }
        };
        let mut poll_rounds = 0_u32;
        let mut subscription_polls = 0_u32;
        loop {
            let tick =
                worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).fuse();
            pin_mut!(tick);
            select! {
                event = websocket_events.next().fuse() => {
                    match event {
                        Some(Ok(WebsocketEvent::Message(message))) => {
                            let Some(text) = message.text() else {
                                let _ = websocket.send_with_str(streaming_websocket_error_message(
                                    "Only text websocket messages are supported",
                                    400,
                                ));
                                continue;
                            };
                            if !handle_streaming_websocket_client_message(
                                &websocket,
                                &mut subscriptions,
                                &text,
                                viewer.as_ref(),
                            ) {
                                break;
                            }
                        }
                        Some(Ok(WebsocketEvent::Close(_))) | None => break,
                        Some(Err(error)) => {
                            console_error!("websocket stream failed: {}", error);
                            break;
                        }
                    }
                }
                _ = tick => {
                    if subscriptions.is_empty() {
                        continue;
                    }
                    let next_subscription_polls = subscription_polls
                        .saturating_add(subscriptions.len() as u32);
                    if streaming_poll_budget_exhausted(poll_rounds, next_subscription_polls) {
                        console_log!(
                            "websocket streaming recycled before subrequest limit rounds={} subscription_polls={} d1_queries={}",
                            poll_rounds,
                            next_subscription_polls,
                            snapshot_d1_request_metrics().query_count
                        );
                        break;
                    }
                    poll_rounds = poll_rounds.saturating_add(1);
                    subscription_polls = next_subscription_polls;
                    if !poll_streaming_websocket_subscriptions(
                        &websocket,
                        &db,
                        &config,
                        viewer.as_ref(),
                        &mut subscriptions,
                    )
                    .await
                    {
                        break;
                    }
                    if streaming_poll_budget_exhausted(poll_rounds, subscription_polls) {
                        console_log!(
                            "websocket streaming recycled before subrequest limit rounds={} subscription_polls={} d1_queries={}",
                            poll_rounds,
                            subscription_polls,
                            snapshot_d1_request_metrics().query_count
                        );
                        break;
                    }
                }
            }
        }
    }
    let _ = websocket.close(Some(1000), Some("stream closed"));
}

/// Resolved StreamHub websocket upgrade target for a Mastodon client.
pub(super) struct StreamHubWebSocketUpgradePlan {
    hub_name: String,
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    account_id: Option<String>,
}

/// Resolve a StreamHub websocket upgrade target.
///
/// When `initial_stream` is set, route using the per-channel mapping. When it is
/// unset but the client is authenticated, route to the account session hub so
/// Mastodon-style post-connect `subscribe` messages are handled by StreamHub.
pub(super) fn stream_hub_websocket_upgrade_plan(
    initial_stream: Option<&str>,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    tag: Option<&str>,
    list: Option<&str>,
) -> Option<StreamHubWebSocketUpgradePlan> {
    if let Some(stream) = initial_stream {
        return stream_hub_proxy_target(stream, viewer, tag, list).map(|(hub_name, account_id)| {
            StreamHubWebSocketUpgradePlan {
                hub_name,
                stream: Some(stream.to_owned()),
                tag: tag.map(str::to_owned),
                list: list.map(str::to_owned),
                account_id,
            }
        });
    }

    viewer.map(|viewer| StreamHubWebSocketUpgradePlan {
        hub_name: stream_hub_session_id_name(viewer.id()),
        stream: None,
        tag: None,
        list: None,
        account_id: Some(viewer.id().to_owned()),
    })
}

pub(super) async fn streaming_websocket_upgrade_response(
    env: &Env,
    req: Request,
    db: crate::D1Database,
    config: cfwdon_core::AppConfig,
    initial_stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    viewer: Option<cfwdon_domain::LocalAccount>,
    websocket_protocol_token: Option<&str>,
) -> Result<Response> {
    if let Some(plan) = stream_hub_websocket_upgrade_plan(
        initial_stream.as_deref(),
        viewer.as_ref(),
        tag.as_deref(),
        list.as_deref(),
    ) {
        let params = StreamHubUpgradeParams {
            stream: plan.stream.as_deref(),
            tag: plan.tag.as_deref(),
            list: plan.list.as_deref(),
            account_id: plan.account_id.as_deref(),
        };
        // The hub reads the subscription from these params, not from anything the
        // client sent, so a forged X-Account-Id or X-Stream cannot take effect.
        match upgrade_stream_hub_websocket(
            env,
            &config.stream_hub_binding,
            &plan.hub_name,
            req,
            &params,
        )
        .await
        {
            Ok(response) => return Ok(response),
            Err(error) => {
                console_log!(
                    "stream hub websocket upgrade failed for hub {}: {:?}; falling back to worker poll",
                    plan.hub_name,
                    error
                );
            }
        }
    } else {
        drop(req);
    }

    let pair = WebSocketPair::new()?;
    pair.server.accept()?;
    let websocket = pair.server.clone();
    spawn_local(async move {
        run_streaming_websocket(websocket, db, config, initial_stream, tag, list, viewer).await;
    });
    let mut response = Response::from_websocket(pair.client)?;
    if let Some(protocol) = websocket_protocol_token {
        response
            .headers_mut()
            .set("Sec-WebSocket-Protocol", protocol)?;
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_websocket_event_message_matches_mastodon_shape() {
        let subscription =
            StreamingWebSocketSubscription::new("user:notification".to_owned(), None, None);
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "notification-1".to_owned(),
            event: "notification",
            data: "{\"id\":\"notification-1\"}".to_owned(),
        };

        let message = streaming_websocket_event_message(&subscription, &event).unwrap();
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(value["stream"], serde_json::json!(["user:notification"]));
        assert_eq!(value["event"], "notification");
        assert_eq!(value["payload"], "{\"id\":\"notification-1\"}");
    }

    #[test]
    fn streaming_websocket_filter_change_omits_payload() {
        let subscription = StreamingWebSocketSubscription::new("user".to_owned(), None, None);
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "filter-change".to_owned(),
            event: "filters_changed",
            data: "undefined".to_owned(),
        };

        let message = streaming_websocket_event_message(&subscription, &event).unwrap();
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(value["stream"], serde_json::json!(["user"]));
        assert_eq!(value["event"], "filters_changed");
        assert!(value.get("payload").is_none());
    }

    #[test]
    fn streaming_websocket_stream_labels_include_subscription_params() {
        assert_eq!(
            streaming_websocket_stream_labels("hashtag", Some("rust"), None),
            vec!["hashtag".to_owned(), "rust".to_owned()]
        );
        assert_eq!(
            streaming_websocket_stream_labels("list", None, Some("list-1")),
            vec!["list".to_owned(), "list-1".to_owned()]
        );
    }

    fn stream_hub_proxy_test_account() -> cfwdon_domain::LocalAccount {
        cfwdon_domain::LocalAccount::from_record(cfwdon_domain::LocalAccountRecord::test_fixture(
            "acct-1", "alice",
        ))
    }

    #[test]
    fn stream_hub_websocket_upgrade_plan_maps_deferred_authenticated_subscribe() {
        let viewer = stream_hub_proxy_test_account();
        let plan = stream_hub_websocket_upgrade_plan(None, Some(&viewer), None, None).unwrap();
        assert_eq!(plan.hub_name, "user:acct-1");
        assert!(plan.stream.is_none());
        assert!(plan.tag.is_none());
        assert!(plan.list.is_none());
        assert_eq!(plan.account_id.as_deref(), Some("acct-1"));
    }

    #[test]
    fn stream_hub_websocket_upgrade_plan_keeps_explicit_stream_mapping() {
        let viewer = stream_hub_proxy_test_account();
        let plan =
            stream_hub_websocket_upgrade_plan(Some("user"), Some(&viewer), None, None).unwrap();
        assert_eq!(plan.hub_name, "user:acct-1");
        assert_eq!(plan.stream.as_deref(), Some("user"));
        assert_eq!(plan.account_id.as_deref(), Some("acct-1"));
    }

    #[test]
    fn stream_hub_websocket_upgrade_plan_defers_anonymous_subscribe_to_worker_poll() {
        assert!(stream_hub_websocket_upgrade_plan(None, None, None, None).is_none());
    }
}
