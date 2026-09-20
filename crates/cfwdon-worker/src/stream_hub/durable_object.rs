use super::client::{
    publish_stream_hub_event_counted, register_stream_hub_forwarding, stream_hub_binding_name,
};
use super::fanout::{stream_hub_fanout_message, stream_hub_websocket_error_message};
use super::logging::{
    log_stream_hub_websocket_event, stream_hub_error_is_deploy_reset,
    stream_hub_error_is_inactive_instance,
};
use super::messages::{
    ForwardRegisterRequest, ForwardTarget, SocketSubscriptionState, StreamHubPublishRequest,
    StreamHubWebSocketClientMessage, StreamSubscription,
};
use super::naming::{
    stream_hub_channel_id_name, stream_hub_session_id_name, stream_is_session_channel,
};
use super::{
    STORAGE_FORWARD_TARGETS_KEY, STREAM_HUB_ACCOUNT_HEADER, STREAM_HUB_FORWARD_REGISTER_PATH,
    STREAM_HUB_LIST_HEADER, STREAM_HUB_PUBLISH_PATH, STREAM_HUB_STREAM_HEADER,
    STREAM_HUB_TAG_HEADER, STREAM_HUB_WEBSOCKET_PATH,
};
use serde::Deserialize;
use worker::{
    DurableObject, Env, Method, Request, Response, Result, State, WebSocket,
    WebSocketIncomingMessage, WebSocketPair, console_error, durable_object,
};

#[durable_object]
pub struct StreamHub {
    state: State,
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

        if req.method() == Method::Post && path.ends_with(STREAM_HUB_FORWARD_REGISTER_PATH) {
            return self.handle_forward_register(req).await;
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
                state.subscriptions.push(subscription.clone());
            }
            if let Some(session_hub) = state.session_hub.clone() {
                self.register_open_channel_forwarding(&subscription, &session_hub)
                    .await;
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
        ws: WebSocket,
        code: usize,
        reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        if stream_hub_error_is_deploy_reset(&reason)
            || stream_hub_error_is_inactive_instance(&reason)
        {
            log_stream_hub_websocket_event("close", &reason);
        }
        let close_code = u16::try_from(code).ok();
        let _ = ws.close(close_code, Some(reason.as_str()));
        Ok(())
    }

    async fn websocket_error(&self, _ws: WebSocket, error: worker::Error) -> Result<()> {
        // worker-rs defaults this to `unimplemented!`, which turns deploy-time
        // Durable Object resets into `$workers.outcome=exception`. Inactive
        // instance errors after hibernation are the same class (#73 residual).
        log_stream_hub_websocket_event("error", &error.to_string());
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

        delivered += self.forward_publish(&publish).await?;

        Response::from_json(&serde_json::json!({ "delivered": delivered }))
    }

    /// Registers a session hub for this open channel so authenticated clients
    /// can mix session and open channels on one socket.
    async fn handle_forward_register(&self, mut req: Request) -> Result<Response> {
        let register = req.json::<ForwardRegisterRequest>().await?;
        let target = ForwardTarget {
            hub: register.hub,
            subscription: StreamSubscription::new(register.stream, register.tag, register.list),
        };

        let storage = self.state.storage();
        let mut targets = storage
            .get::<Vec<ForwardTarget>>(STORAGE_FORWARD_TARGETS_KEY)
            .await?
            .unwrap_or_default();
        if !targets.contains(&target) {
            targets.push(target);
            storage.put(STORAGE_FORWARD_TARGETS_KEY, &targets).await?;
        }

        Response::from_json(&serde_json::json!({ "targets": targets.len() }))
    }

    /// Relays an event to every session hub registered for it. Targets that no
    /// longer have a matching subscriber are dropped, so disconnects need no
    /// explicit unregister step.
    async fn forward_publish(&self, publish: &StreamHubPublishRequest) -> Result<usize> {
        let storage = self.state.storage();
        let mut targets = storage
            .get::<Vec<ForwardTarget>>(STORAGE_FORWARD_TARGETS_KEY)
            .await?
            .unwrap_or_default();
        if targets.is_empty() {
            return Ok(0);
        }

        let binding = stream_hub_binding_name(&self.env);
        let body = publish.to_body();
        let mut delivered = 0usize;
        let mut stale = Vec::new();

        for target in &targets {
            if !target.subscription.matches(
                &publish.stream,
                publish.tag.as_deref(),
                publish.list.as_deref(),
            ) {
                continue;
            }
            match publish_stream_hub_event_counted(&self.env, &binding, &target.hub, &body).await {
                Ok(0) => stale.push(target.clone()),
                Ok(count) => delivered += count,
                Err(error) => {
                    console_error!(
                        "failed to forward stream hub event to session hub {}: {error}",
                        target.hub
                    );
                    stale.push(target.clone());
                }
            }
        }

        if !stale.is_empty() {
            targets.retain(|target| !stale.contains(target));
            storage.put(STORAGE_FORWARD_TARGETS_KEY, &targets).await?;
        }

        Ok(delivered)
    }

    /// Asks the open channel's hub to forward its events to this session hub.
    async fn register_open_channel_forwarding(
        &self,
        subscription: &StreamSubscription,
        session_hub: &str,
    ) {
        if stream_is_session_channel(&subscription.stream) {
            return;
        }
        let channel_hub =
            stream_hub_channel_id_name(&subscription.stream, subscription.tag.as_deref());
        if channel_hub == session_hub {
            return;
        }
        let binding = stream_hub_binding_name(&self.env);
        if let Err(error) = register_stream_hub_forwarding(
            &self.env,
            &binding,
            &channel_hub,
            session_hub,
            subscription,
        )
        .await
        {
            console_error!(
                "failed to register forwarding of {} to session hub {session_hub}: {error}",
                subscription.stream
            );
        }
    }

    async fn handle_websocket_upgrade(&self, req: Request) -> Result<Response> {
        let params = stream_hub_connect_params(&req)?;
        let pair = WebSocketPair::new()?;

        // Accept-time tags cannot express the subscription set: clients add and
        // drop channels over the life of the socket, and tags are fixed at
        // accept. Routing therefore reads the attachment instead.
        self.state.accept_web_socket(&pair.server);

        let session_hub = params
            .account_id
            .as_deref()
            .map(str::trim)
            .filter(|account_id| !account_id.is_empty())
            .map(stream_hub_session_id_name);
        let subscriptions = params
            .stream
            .map(|stream| vec![StreamSubscription::new(stream, params.tag, params.list)])
            .unwrap_or_default();
        let state = SocketSubscriptionState {
            subscriptions,
            session_hub,
        };
        pair.server.serialize_attachment(&state)?;

        if let Some(session_hub) = state.session_hub.clone() {
            for subscription in &state.subscriptions {
                self.register_open_channel_forwarding(subscription, &session_hub)
                    .await;
            }
        }

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

/// The Worker authenticates the connection and states the subscription in
/// internal headers. Those win over the query string, which is client-supplied
/// and forwarded verbatim on proxied upgrades.
fn stream_hub_connect_params(req: &Request) -> Result<StreamHubConnectParams> {
    let query = req.query::<StreamHubConnectParams>().unwrap_or_default();

    Ok(StreamHubConnectParams {
        stream: header_value(req, STREAM_HUB_STREAM_HEADER).or(query.stream),
        tag: header_value(req, STREAM_HUB_TAG_HEADER).or(query.tag),
        list: header_value(req, STREAM_HUB_LIST_HEADER).or(query.list),
        account_id: header_value(req, STREAM_HUB_ACCOUNT_HEADER).or(query.account_id),
    })
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
