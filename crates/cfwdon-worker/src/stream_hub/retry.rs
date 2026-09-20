use super::logging::{log_stream_hub_connect_event, stream_hub_error_is_inactive_instance};
use worker::{Env, Request, Response, Result};

/// Initial connect plus one retry when the target DO was hibernated/evicted.
pub(super) const STREAM_HUB_INACTIVE_FETCH_ATTEMPTS: u8 = 2;

/// After a live Worker↔DO websocket dies, reconnect this many times before D1 poll.
pub(crate) const STREAM_HUB_INACTIVE_SSE_RECONNECTS: u8 = 2;

pub(crate) fn stream_hub_sse_should_reconnect(detail: Option<&str>, reconnects_used: u8) -> bool {
    if reconnects_used >= STREAM_HUB_INACTIVE_SSE_RECONNECTS {
        return false;
    }
    match detail {
        None => true,
        Some(message) => stream_hub_error_is_inactive_instance(message),
    }
}

pub(super) async fn fetch_stream_hub_with_inactive_retry(
    env: &Env,
    binding: &str,
    hub_name: &str,
    operation: &'static str,
    build_request: impl Fn() -> Result<Request>,
) -> Result<Response> {
    fetch_stream_hub_with_inactive_retry_map(env, binding, hub_name, operation, build_request, Ok)
        .await
}

pub(super) async fn fetch_stream_hub_with_inactive_retry_map<T>(
    env: &Env,
    binding: &str,
    hub_name: &str,
    operation: &'static str,
    build_request: impl Fn() -> Result<Request>,
    map_response: impl Fn(Response) -> Result<T>,
) -> Result<T> {
    for attempt in 1..=STREAM_HUB_INACTIVE_FETCH_ATTEMPTS {
        let namespace = env.durable_object(binding)?;
        let stub = namespace.get_by_name(hub_name)?;
        let request = build_request()?;
        let mapped = match stub.fetch_with_request(request).await {
            Ok(response) => map_response(response),
            Err(error) => Err(error),
        };
        match mapped {
            Ok(value) => {
                if attempt > 1 {
                    log_stream_hub_connect_event(operation, "inactive_retry_ok", attempt, None);
                }
                return Ok(value);
            }
            Err(error) => {
                let message = error.to_string();
                if !stream_hub_error_is_inactive_instance(&message) {
                    return Err(error);
                }
                if attempt < STREAM_HUB_INACTIVE_FETCH_ATTEMPTS {
                    log_stream_hub_connect_event(
                        operation,
                        "inactive_retry",
                        attempt,
                        Some(&message),
                    );
                    continue;
                }
                log_stream_hub_connect_event(
                    operation,
                    "inactive_retry_exhausted",
                    attempt,
                    Some(&message),
                );
                return Err(error);
            }
        }
    }
    Err(worker::Error::RustError(
        "stream hub inactive retry loop exited without a response".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_sse_reconnects_on_close_or_inactive_error() {
        assert!(stream_hub_sse_should_reconnect(None, 0));
        assert!(stream_hub_sse_should_reconnect(
            Some(
                "Connection closed: this Durable Object instance is no longer active. Reconnect or retry the request."
            ),
            1
        ));
        assert!(!stream_hub_sse_should_reconnect(
            Some(
                "Connection closed: this Durable Object instance is no longer active. Reconnect or retry the request."
            ),
            STREAM_HUB_INACTIVE_SSE_RECONNECTS
        ));
        assert!(!stream_hub_sse_should_reconnect(
            Some("websocket closed"),
            0
        ));
    }
}
