use crate::{add_log_message, log_json_event};

pub(crate) fn stream_hub_error_is_inactive_instance(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("no longer active")
}

pub(super) fn stream_hub_error_is_deploy_reset(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("durable object reset") && message.contains("updated")
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

pub(super) fn log_stream_hub_websocket_event(handler: &str, detail: &str) {
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
    log_json_event(add_log_message(
        serde_json::json!({
            "event": "stream_hub_websocket",
            "component": "stream_hub",
            "handler": handler,
            "outcome": outcome,
            "level": level,
            "deploy_reset": deploy_reset,
            "inactive_instance": inactive_instance,
        }),
        message,
    ));
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
}
