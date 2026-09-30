use crate::observability::add_log_message;

use crate::observability::log_json_event;

pub(crate) fn stream_hub_error_is_inactive_instance(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("no longer active")
}

pub(super) fn stream_hub_error_is_deploy_reset(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("durable object reset") && message.contains("updated")
}

/// Close code to echo from `webSocketClose`, if echoing is safe.
///
/// Hibernation eviction is delivered as code 1006 with an inactive-instance
/// reason. workerd rejects 1004/1005/1006/1015 in `WebSocket.close`, and a
/// close queued onto an already-dead hibernatable socket fails in the output
/// pump after this handler has returned. That rejection is what
/// `HibernatableWebSocketCustomEvent::run` records as
/// `$workers.outcome=exception`. Skipping the reply lets the handler resolve.
/// Compatibility date `2026-04-13` auto-replies to ordinary Close frames, so
/// the skipped cases do not need an application close.
pub(super) fn stream_hub_websocket_close_reply_code(code: usize, reason: &str) -> Option<u16> {
    if stream_hub_error_is_deploy_reset(reason) || stream_hub_error_is_inactive_instance(reason) {
        return None;
    }
    let code = u16::try_from(code).ok()?;
    if !(1000..5000).contains(&code) || matches!(code, 1004 | 1005 | 1006 | 1015) {
        return None;
    }
    Some(code)
}

pub(super) fn stream_hub_websocket_log_class(
    handler: &str,
    detail: &str,
) -> (&'static str, &'static str) {
    if stream_hub_error_is_deploy_reset(detail) {
        ("info", "deploy_reset")
    } else if stream_hub_error_is_inactive_instance(detail) {
        ("warn", "inactive_instance")
    } else if handler == "error" {
        ("warn", "error")
    } else {
        ("info", "closed")
    }
}

pub(crate) fn log_stream_hub_connect_event(
    operation: &str,
    outcome: &str,
    attempt: u8,
    detail: Option<&str>,
) {
    let level = match outcome {
        "inactive_retry" | "inactive_retry_exhausted" => "warn",
        _ => "info",
    };
    let message = match (outcome, detail) {
        ("inactive_retry", Some(detail)) => {
            format!(
                "Stream Hub {operation} retrying inactive Durable Object (attempt {attempt}): {detail}"
            )
        }
        ("inactive_retry_ok", _) => {
            format!("Stream Hub {operation} succeeded after inactive Durable Object retry")
        }
        ("inactive_retry_exhausted", Some(detail)) => {
            format!(
                "Stream Hub {operation} exhausted inactive Durable Object retries (attempt {attempt}): {detail}"
            )
        }
        (_, Some(detail)) => format!("Stream Hub {operation} {outcome}: {detail}"),
        (_, None) => format!("Stream Hub {operation} {outcome}"),
    };
    log_json_event(add_log_message(
        serde_json::json!({
            "event": "stream_hub_websocket",
            "component": "stream_hub",
            "handler": operation,
            "outcome": outcome,
            "level": level,
            "attempt": attempt,
            "inactive_instance": true,
        }),
        message,
    ));
}

pub(super) fn log_stream_hub_websocket_event(
    handler: &str,
    detail: &str,
    close_reply: Option<&str>,
) {
    let deploy_reset = stream_hub_error_is_deploy_reset(detail);
    let inactive_instance = stream_hub_error_is_inactive_instance(detail);
    let (level, outcome) = stream_hub_websocket_log_class(handler, detail);
    let message = if deploy_reset {
        "Stream Hub websocket closed because Durable Object code was updated".to_owned()
    } else if inactive_instance {
        format!(
            "Stream Hub websocket {handler} because Durable Object instance is no longer active"
        )
    } else {
        format!("Stream Hub websocket {handler}: {detail}")
    };
    // `handled` marks an application log. It does not clear Workers
    // `$metadata.error`: that field is written by the runtime when the
    // hibernatable event's IoContext is already aborted, and no handler
    // return value rewrites `$workers.outcome`.
    let mut payload = serde_json::json!({
        "event": "stream_hub_websocket",
        "component": "stream_hub",
        "handler": handler,
        "outcome": outcome,
        "level": level,
        "handled": true,
        "deploy_reset": deploy_reset,
        "inactive_instance": inactive_instance,
    });
    if let (Some(close_reply), Some(object)) = (close_reply, payload.as_object_mut()) {
        object.insert(
            "close_reply".to_owned(),
            serde_json::Value::String(close_reply.to_owned()),
        );
    }
    log_json_event(add_log_message(payload, message));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploy_reset_websocket_errors_are_operational_not_exceptions() {
        assert!(stream_hub_error_is_deploy_reset(
            "Durable Object reset because its code was updated."
        ));
        assert!(stream_hub_error_is_deploy_reset(
            "Error: Durable Object reset because its code was updated."
        ));
        assert!(!stream_hub_error_is_deploy_reset("websocket closed"));
        assert!(!stream_hub_error_is_deploy_reset(
            "failed to forward stream hub event"
        ));
    }

    #[test]
    fn inactive_instance_errors_are_retried() {
        assert!(stream_hub_error_is_inactive_instance(
            "Connection closed: this Durable Object instance is no longer active. Reconnect or retry the request."
        ));
        assert!(stream_hub_error_is_inactive_instance(
            "Error: this Durable Object instance is no longer active"
        ));
        assert!(!stream_hub_error_is_inactive_instance(
            "Durable Object reset because its code was updated."
        ));
        assert!(!stream_hub_error_is_inactive_instance("websocket closed"));
    }

    #[test]
    fn inactive_websocket_errors_are_operational_not_exceptions() {
        assert_eq!(
            stream_hub_websocket_log_class(
                "error",
                "Connection closed: this Durable Object instance is no longer active. Reconnect or retry the request."
            ),
            ("warn", "inactive_instance")
        );
        assert_eq!(
            stream_hub_websocket_log_class("error", "socket failed"),
            ("warn", "error")
        );
        assert_eq!(
            stream_hub_websocket_log_class(
                "close",
                "Durable Object reset because its code was updated."
            ),
            ("info", "deploy_reset")
        );
    }

    #[test]
    fn inactive_and_illegal_close_codes_are_not_echoed() {
        let inactive = "Connection closed: this Durable Object instance is no longer active. Reconnect or retry the request.";
        assert_eq!(stream_hub_websocket_close_reply_code(1006, inactive), None);
        assert_eq!(stream_hub_websocket_close_reply_code(1000, inactive), None);
        assert_eq!(
            stream_hub_websocket_close_reply_code(
                1006,
                "Durable Object reset because its code was updated."
            ),
            None
        );
        assert_eq!(stream_hub_websocket_close_reply_code(1006, ""), None);
        assert_eq!(stream_hub_websocket_close_reply_code(1005, ""), None);
        assert_eq!(stream_hub_websocket_close_reply_code(1004, ""), None);
        assert_eq!(stream_hub_websocket_close_reply_code(1015, ""), None);
        assert_eq!(stream_hub_websocket_close_reply_code(999, ""), None);
        assert_eq!(
            stream_hub_websocket_close_reply_code(1000, "client closed"),
            Some(1000)
        );
        assert_eq!(
            stream_hub_websocket_close_reply_code(1001, "going away"),
            Some(1001)
        );
    }
}
