use super::client::{
    publish_stream_hub_event_counted, register_stream_hub_forwarding, stream_hub_binding_name,
};
use super::fanout::{stream_hub_fanout_message, stream_hub_websocket_error_message};
use super::logging::{
    log_stream_hub_websocket_event, stream_hub_error_is_deploy_reset,
    stream_hub_error_is_inactive_instance, stream_hub_websocket_close_reply_code,
};
use super::messages::{
    ForwardRegisterRequest, ForwardTarget, SocketSubscriptionState, StreamHubPublishRequest,
    StreamHubWebSocketClientMessage, StreamSubscription,
};
use super::naming::{
    stream_hub_channel_id_name, stream_hub_session_id_name, stream_is_session_channel,
};
use super::registry::{hub_registry_keys, replace_hub_registry_rows};
use super::{
    STORAGE_FORWARD_TARGETS_KEY, STREAM_HUB_ACCOUNT_HEADER, STREAM_HUB_FORWARD_REGISTER_PATH,
    STREAM_HUB_LIST_HEADER, STREAM_HUB_NAME_HEADER, STREAM_HUB_PUBLISH_PATH,
    STREAM_HUB_STREAM_HEADER, STREAM_HUB_TAG_HEADER, STREAM_HUB_WEBSOCKET_PATH,
};
use crate::runtime_config::load_config_from_env;
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use std::cell::Cell;
use worker::{
    DurableObject, Env, Method, Request, Response, Result, State, WebSocket,
    WebSocketIncomingMessage, WebSocketPair, console_error, durable_object,
};

#[durable_object]
pub struct StreamHub {
    state: State,
    env: Env,
    registry_syncing: Cell<bool>,
    registry_stale: Cell<bool>,
}

impl DurableObject for StreamHub {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            env,
            registry_syncing: Cell::new(false),
            registry_stale: Cell::new(false),
        }
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
        if let Some(hub) = state.hub.as_deref() {
            self.sync_registry(hub).await;
        }
        Ok(())
    }

    async fn websocket_close(
        &self,
        ws: WebSocket,
        code: usize,
        reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        let hub = ws
            .deserialize_attachment::<SocketSubscriptionState>()
            .ok()
            .flatten()
            .and_then(|state| state.hub);
        // The closing socket may still be listed by `get_websockets`; drop its
        // subscriptions so the registry sync below no longer counts them.
        let _ = ws.serialize_attachment(SocketSubscriptionState::default());
        // Eviction arrives as 1006 / "no longer active". Echoing that close
        // either throws immediately or fails the hibernatable output pump
        // after this function returns Ok, and workerd then sets
        // `$workers.outcome=exception`. `let _ = ws.close()` only catches the
        // synchronous error. Skip the reply instead.
        let close_reply = match stream_hub_websocket_close_reply_code(code, &reason) {
            Some(close_code) => {
                let _ = ws.close(Some(close_code), Some(reason.as_str()));
                "sent"
            }
            None => "skipped",
        };
        if stream_hub_error_is_deploy_reset(&reason)
            || stream_hub_error_is_inactive_instance(&reason)
        {
            log_stream_hub_websocket_event("close", &reason, Some(close_reply));
        }
        if let Some(hub) = hub.as_deref() {
            self.sync_registry(hub).await;
        }
        Ok(())
    }

    async fn websocket_error(&self, _ws: WebSocket, error: worker::Error) -> Result<()> {
        // worker-rs defaults this to `unimplemented!`, which turns deploy-time
        // Durable Object resets into `$workers.outcome=exception`. Returning
        // Ok handles the error when the handler actually runs. It does not
        // clear `$metadata.error` if the runtime aborted the IoContext before
        // this method was entered (#96).
        log_stream_hub_websocket_event("error", &error.to_string(), None);
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

        // The Worker publishes only to hubs the registry lists, so nobody
        // receiving it means this hub's rows outlived their sockets.
        if delivered == 0
            && let Some(hub) = header_value(&req, STREAM_HUB_NAME_HEADER)
        {
            self.sync_registry(&hub).await;
        }

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

    /// Replaces this hub's registry rows with what its live sockets hold. Runs
    /// one write at a time; a change during a write triggers one more pass, so
    /// the last write reflects the latest sockets.
    async fn sync_registry(&self, hub: &str) {
        if self.registry_syncing.replace(true) {
            self.registry_stale.set(true);
            return;
        }
        loop {
            self.registry_stale.set(false);
            let sockets = self
                .state
                .get_websockets()
                .iter()
                .filter_map(|socket| {
                    socket
                        .deserialize_attachment::<SocketSubscriptionState>()
                        .ok()
                        .flatten()
                })
                .collect::<Vec<_>>();
            let keys = hub_registry_keys(hub, &sockets);
            let written = match self
                .env
                .d1(&load_config_from_env(&self.env).database_binding)
            {
                Ok(db) => replace_hub_registry_rows(&D1Database::new(db), hub, &keys).await,
                Err(error) => Err(error),
            };
            if let Err(error) = written {
                console_error!("failed to update stream subscriptions of hub {hub}: {error}");
            }
            if !self.registry_stale.get() {
                break;
            }
        }
        self.registry_syncing.set(false);
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
            hub: header_value(&req, STREAM_HUB_NAME_HEADER),
        };
        pair.server.serialize_attachment(&state)?;
        if let Some(hub) = state.hub.as_deref()
            && !state.subscriptions.is_empty()
        {
            self.sync_registry(hub).await;
        }

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
