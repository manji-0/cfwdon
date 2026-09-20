use super::STREAMING_POLL_INTERVAL_SECS;
use super::channels::streaming_channel_supports_live_events;
use super::hub_routing::stream_hub_proxy_target;
use super::poll::{StreamingPollYield, poll_streaming_events, yield_streaming_poll_round};
use crate::{
    D1Database, Response, Result, StreamingEvent, StreamingLoopState, connect_stream_hub_websocket,
    log_stream_hub_connect_event, stream_hub_sse_should_reconnect,
};
use async_stream::try_stream;
use futures_util::{FutureExt, StreamExt, pin_mut, select};
use std::time::Duration;
use worker::{Env, ResponseBody, WebSocket, console_error, console_log, ws_events::WebsocketEvent};

pub(super) const STREAMING_HUB_BACKUP_POLL_INTERVAL_SECS: u64 = 30;

pub(super) const STREAMING_SSE_HUB_CLOSE_CODE: u16 = 1000;

pub(super) const STREAMING_SSE_HUB_CLOSE_REASON: &str = "sse closed";

pub(super) fn sse_comment_bytes(value: &str) -> Vec<u8> {
    format!(": {value}\n\n").into_bytes()
}

pub(super) fn sse_event_bytes(event: &StreamingEvent) -> Vec<u8> {
    format!("event: {}\ndata: {}\n\n", event.event, event.data).into_bytes()
}

pub(super) fn sse_named_event_bytes(event: &str, data: &str) -> Vec<u8> {
    format!("event: {event}\ndata: {data}\n\n").into_bytes()
}

pub(super) fn streaming_event_identity_from_payload(
    event_name: &str,
    data: &str,
) -> (String, String) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
        let id = value
            .get("id")
            .and_then(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| value.as_i64().map(|id| id.to_string()))
            })
            .unwrap_or_else(|| format!("{event_name}:{data}"));
        let created_at = value
            .get("created_at")
            .and_then(|value| value.as_str())
            .unwrap_or("1970-01-01T00:00:00Z")
            .to_owned();
        return (id, created_at);
    }

    (
        format!("{event_name}:{}", data.chars().take(64).collect::<String>()),
        "1970-01-01T00:00:00Z".to_owned(),
    )
}

pub(super) fn stream_hub_websocket_text_to_sse_bytes(
    text: &str,
    state: &mut StreamingLoopState,
) -> Option<Vec<u8>> {
    let value = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => value,
        Err(_) => return None,
    };
    if value.get("error").is_some() {
        return None;
    }
    let event_name = value
        .get("event")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if event_name.is_empty() {
        return None;
    }
    // `filters_changed` carries no payload to dedupe on, and the D1 catch-up
    // cursor only advances every backup poll, so keying it on that cursor would
    // drop every change after the first. Pass it through unconditionally.
    if event_name == "filters_changed" {
        return Some(sse_named_event_bytes(event_name, "undefined"));
    }

    let data = value
        .get("payload")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned();
    let (id, _created_at) = streaming_event_identity_from_payload(event_name, &data);
    let dedupe_key = format!("{event_name}:{id}");
    if !state.emitted_event_ids.insert(dedupe_key) {
        return None;
    }
    Some(sse_named_event_bytes(event_name, &data))
}

/// Closes a Worker↔StreamHub websocket when public SSE is cancelled.
///
/// The accepted hub socket is not closed by `WebSocket` Drop. If it stays open
/// after the client disconnects, the runtime treats that I/O as `waitUntil`
/// work and logs a 30s cancellation warning (#52).
pub(super) struct CloseWebSocketOnDrop {
    websocket: Option<WebSocket>,
}

impl CloseWebSocketOnDrop {
    fn new(websocket: WebSocket) -> Self {
        Self {
            websocket: Some(websocket),
        }
    }

    fn events(&self) -> Option<worker::EventStream<'_>> {
        self.websocket
            .as_ref()
            .and_then(|websocket| websocket.events().ok())
    }
}

impl Drop for CloseWebSocketOnDrop {
    fn drop(&mut self) {
        if let Some(websocket) = self.websocket.take() {
            let _ = websocket.close(
                Some(STREAMING_SSE_HUB_CLOSE_CODE),
                Some(STREAMING_SSE_HUB_CLOSE_REASON),
            );
        }
    }
}

pub(super) fn build_streaming_event_stream(
    env: Option<Env>,
    db: D1Database,
    config: cfwdon_core::AppConfig,
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
    viewer: Option<crate::LocalAccount>,
    hub_target: Option<(String, Option<String>)>,
) -> impl futures_util::TryStream<
    Ok = Vec<u8>,
    Error = worker::Error,
    Item = std::result::Result<Vec<u8>, worker::Error>,
> + 'static {
    try_stream! {
        yield sse_comment_bytes(&format!("stream={stream_name}"));
        let mut state = StreamingLoopState::new();
        let mut poll_rounds = 0_u32;
        let mut hub_socket = if let (Some(env), Some((hub_name, account_id))) =
            (env.as_ref(), hub_target.as_ref())
        {
            yield sse_comment_bytes("source=stream-hub");
            match connect_stream_hub_websocket(
                env,
                &config.stream_hub_binding,
                hub_name,
                &stream_name,
                tag.as_deref(),
                list.as_deref(),
                account_id.as_deref(),
            )
            .await
            {
                Ok(websocket) => {
                    if let Err(error) = poll_streaming_events(
                        &db,
                        &config,
                        &stream_name,
                        tag.as_deref(),
                        list.as_deref(),
                        viewer.as_ref(),
                        &mut state,
                    )
                    .await
                    {
                        console_log!(
                            "stream hub sse initial sync failed stream={} error={}",
                            stream_name,
                            error
                        );
                    }
                    Some(CloseWebSocketOnDrop::new(websocket))
                }
                Err(error) => {
                    console_log!(
                        "stream hub sse connect failed for hub {}: {:?}; falling back to d1 poll",
                        hub_name,
                        error
                    );
                    None
                }
            }
        } else {
            None
        };

        let mut hub_reconnects = 0_u8;
        let mut hub_events = hub_socket.as_ref().and_then(CloseWebSocketOnDrop::events);
        while hub_events.is_some() {
            let backup_tick =
                worker::Delay::from(Duration::from_secs(STREAMING_HUB_BACKUP_POLL_INTERVAL_SECS))
                    .fuse();
            pin_mut!(backup_tick);
            select! {
                event = hub_events.as_mut().unwrap().next().fuse() => {
                    match event {
                        Some(Ok(WebsocketEvent::Message(message))) => {
                            if let Some(bytes) = message
                                .text()
                                .and_then(|text| {
                                    stream_hub_websocket_text_to_sse_bytes(&text, &mut state)
                                })
                            {
                                yield bytes;
                            }
                        }
                        Some(Ok(WebsocketEvent::Close(_))) | None | Some(Err(_)) => {
                            let error_detail = match &event {
                                Some(Err(error)) => Some(error.to_string()),
                                _ => None,
                            };
                            let should_reconnect = stream_hub_sse_should_reconnect(
                                error_detail.as_deref(),
                                hub_reconnects,
                            );
                            // Drop listeners before closing the socket (EventStream panics if
                            // the websocket is already gone).
                            hub_events = None;
                            drop(hub_socket.take());
                            if should_reconnect {
                                if let (Some(env), Some((hub_name, account_id))) =
                                    (env.as_ref(), hub_target.as_ref())
                                {
                                    hub_reconnects = hub_reconnects.saturating_add(1);
                                    log_stream_hub_connect_event(
                                        "sse",
                                        "inactive_retry",
                                        hub_reconnects,
                                        error_detail.as_deref(),
                                    );
                                    match connect_stream_hub_websocket(
                                        env,
                                        &config.stream_hub_binding,
                                        hub_name,
                                        &stream_name,
                                        tag.as_deref(),
                                        list.as_deref(),
                                        account_id.as_deref(),
                                    )
                                    .await
                                    {
                                        Ok(websocket) => {
                                            log_stream_hub_connect_event(
                                                "sse",
                                                "inactive_retry_ok",
                                                hub_reconnects,
                                                None,
                                            );
                                            hub_socket = Some(CloseWebSocketOnDrop::new(websocket));
                                            hub_events = hub_socket
                                                .as_ref()
                                                .and_then(CloseWebSocketOnDrop::events);
                                        }
                                        Err(error) => {
                                            log_stream_hub_connect_event(
                                                "sse",
                                                "inactive_retry_exhausted",
                                                hub_reconnects,
                                                Some(&error.to_string()),
                                            );
                                            console_log!(
                                                "stream hub sse reconnect failed for hub {}; falling back to d1 poll",
                                                hub_name
                                            );
                                        }
                                    }
                                }
                            } else if let Some((hub_name, _)) = hub_target.as_ref() {
                                match error_detail {
                                    Some(detail) => console_error!(
                                        "stream hub sse websocket error for hub {}: {}",
                                        hub_name,
                                        detail
                                    ),
                                    None => console_log!(
                                        "stream hub sse websocket closed for hub {}; falling back to d1 poll",
                                        hub_name
                                    ),
                                }
                            }
                        }
                    }
                }
                _ = backup_tick => {
                    match yield_streaming_poll_round(
                        &db,
                        &config,
                        &stream_name,
                        tag.as_deref(),
                        list.as_deref(),
                        viewer.as_ref(),
                        &mut state,
                        &mut poll_rounds,
                    )
                    .await
                    {
                        StreamingPollYield::Events(events) => {
                            if events.is_empty() {
                                yield sse_comment_bytes("thump");
                            } else {
                                for event in events {
                                    yield sse_event_bytes(&event);
                                }
                            }
                        }
                        StreamingPollYield::PollFailed => {
                            yield sse_comment_bytes("error=streaming_poll_failed");
                        }
                        StreamingPollYield::Recycle => {
                            yield sse_comment_bytes("stream=recycle");
                            return;
                        }
                    }
                }
            }
        }

        loop {
            match yield_streaming_poll_round(
                &db,
                &config,
                &stream_name,
                tag.as_deref(),
                list.as_deref(),
                viewer.as_ref(),
                &mut state,
                &mut poll_rounds,
            )
            .await
            {
                StreamingPollYield::Events(events) => {
                    if events.is_empty() {
                        yield sse_comment_bytes("thump");
                    } else {
                        for event in events {
                            yield sse_event_bytes(&event);
                        }
                    }
                }
                StreamingPollYield::PollFailed => {
                    yield sse_comment_bytes("error=streaming_poll_failed");
                    worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).await;
                    continue;
                }
                StreamingPollYield::Recycle => {
                    yield sse_comment_bytes("stream=recycle");
                    break;
                }
            }
            worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).await;
        }
    }
}

pub(super) fn streaming_sse_response(
    env: &Env,
    db: crate::D1Database,
    config: cfwdon_core::AppConfig,
    stream: String,
    tag: Option<String>,
    list: Option<String>,
    viewer: Option<cfwdon_domain::LocalAccount>,
) -> Result<Response> {
    if streaming_channel_supports_live_events(&stream) {
        let hub_target =
            stream_hub_proxy_target(&stream, viewer.as_ref(), tag.as_deref(), list.as_deref());
        let env_for_hub = if hub_target.is_some() {
            Some(env.clone())
        } else {
            None
        };
        let stream_body = build_streaming_event_stream(
            env_for_hub,
            db,
            config,
            stream,
            tag,
            list,
            viewer,
            hub_target,
        );
        let mut response = Response::from_stream(stream_body)?;
        response
            .headers_mut()
            .set("Content-Type", "text/event-stream")?;
        response.headers_mut().set("Cache-Control", "no-cache")?;
        return Ok(response);
    }

    let mut body = format!(": cfwdon-placeholder stream={stream}\n");
    if let Some(tag) = tag
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.push_str(&format!(": tag={tag}\n"));
    }
    if let Some(list) = list
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.push_str(&format!(": list={list}\n"));
    }
    body.push('\n');
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/event-stream")?;
    response.headers_mut().set("Cache-Control", "no-cache")?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_event_bytes_match_event_stream_format() {
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "event-1".to_owned(),
            event: "update",
            data: "{\"id\":\"status-1\"}".to_owned(),
        };

        assert_eq!(
            sse_event_bytes(&event),
            b"event: update\ndata: {\"id\":\"status-1\"}\n\n".to_vec()
        );
    }

    #[test]
    fn stream_hub_websocket_text_to_sse_bytes_matches_event_stream_format() {
        let mut state = StreamingLoopState::new();
        let message = serde_json::json!({
            "stream": ["user"],
            "event": "update",
            "payload": "{\"id\":\"status-1\"}",
        });

        assert_eq!(
            stream_hub_websocket_text_to_sse_bytes(&message.to_string(), &mut state),
            Some(b"event: update\ndata: {\"id\":\"status-1\"}\n\n".to_vec())
        );
    }

    #[test]
    fn stream_hub_websocket_text_to_sse_bytes_dedupes_repeated_events() {
        let mut state = StreamingLoopState::new();
        let message = serde_json::json!({
            "stream": ["user"],
            "event": "update",
            "payload": "{\"id\":\"status-1\"}",
        })
        .to_string();

        assert!(stream_hub_websocket_text_to_sse_bytes(&message, &mut state).is_some());
        assert!(stream_hub_websocket_text_to_sse_bytes(&message, &mut state).is_none());
    }

    #[test]
    fn stream_hub_websocket_text_to_sse_bytes_handles_filters_changed() {
        let mut state = StreamingLoopState::new();
        state.last_filter_updated_at = Some("2025-01-02T00:00:00Z".to_owned());
        let message = serde_json::json!({
            "stream": ["user"],
            "event": "filters_changed",
        });

        assert_eq!(
            stream_hub_websocket_text_to_sse_bytes(&message.to_string(), &mut state),
            Some(b"event: filters_changed\ndata: undefined\n\n".to_vec())
        );
        // Repeated filter edits must keep arriving.
        assert_eq!(
            stream_hub_websocket_text_to_sse_bytes(&message.to_string(), &mut state),
            Some(b"event: filters_changed\ndata: undefined\n\n".to_vec())
        );
    }

    #[test]
    fn stream_hub_sse_close_uses_normal_closure() {
        assert_eq!(STREAMING_SSE_HUB_CLOSE_CODE, 1000);
        assert_eq!(STREAMING_SSE_HUB_CLOSE_REASON, "sse closed");
    }
}
