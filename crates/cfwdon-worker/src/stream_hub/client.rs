use super::messages::StreamSubscription;
use super::retry::fetch_stream_hub_with_inactive_retry;
use super::{
    STREAM_HUB_BINDING_VAR, STREAM_HUB_DEFAULT_BINDING, STREAM_HUB_FORWARD_REGISTER_PATH,
    STREAM_HUB_INTERNAL_ORIGIN, STREAM_HUB_PUBLISH_PATH,
};
use serde::Deserialize;
use worker::{Env, Method, Request, RequestInit, Response, Result};

#[derive(Debug, Default, Deserialize)]
pub(super) struct StreamHubPublishResponse {
    #[serde(default)]
    pub(super) delivered: usize,
}

/// Binding used for hub-to-hub forwarding. Falls back to the default name when
/// the optional `STREAM_HUB_BINDING` var is unset.
pub(super) fn stream_hub_binding_name(env: &Env) -> String {
    env.var(STREAM_HUB_BINDING_VAR)
        .ok()
        .map(|value| value.to_string())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| STREAM_HUB_DEFAULT_BINDING.to_owned())
}

pub(super) async fn post_json_to_stream_hub(
    env: &Env,
    binding: &str,
    hub_name: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<Response> {
    let body_json = serde_json::to_string(body).map_err(|error| {
        worker::Error::RustError(format!("failed to encode stream hub request body: {error}"))
    })?;
    let path = path.to_owned();

    fetch_stream_hub_with_inactive_retry(env, binding, hub_name, "publish", || {
        let headers = worker::Headers::new();
        headers.set("Content-Type", "application/json")?;

        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&body_json)));

        let url = format!("{STREAM_HUB_INTERNAL_ORIGIN}{path}");
        Request::new_with_init(&url, &init)
    })
    .await
}

/// Publishes to one hub and reports how many subscribers received the event.
pub(crate) async fn publish_stream_hub_event_counted(
    env: &Env,
    binding: &str,
    hub_name: &str,
    body: &serde_json::Value,
) -> Result<usize> {
    let mut response =
        post_json_to_stream_hub(env, binding, hub_name, STREAM_HUB_PUBLISH_PATH, body).await?;
    if response.status_code() >= 400 {
        return Err(worker::Error::RustError(format!(
            "stream hub publish failed with HTTP {}",
            response.status_code()
        )));
    }
    Ok(response
        .json::<StreamHubPublishResponse>()
        .await
        .unwrap_or_default()
        .delivered)
}

pub(crate) async fn publish_stream_hub_event(
    env: &Env,
    binding: &str,
    hub_name: &str,
    body: &serde_json::Value,
) -> Result<()> {
    publish_stream_hub_event_counted(env, binding, hub_name, body)
        .await
        .map(|_| ())
}

pub(super) async fn register_stream_hub_forwarding(
    env: &Env,
    binding: &str,
    channel_hub: &str,
    session_hub: &str,
    subscription: &StreamSubscription,
) -> Result<()> {
    let body = serde_json::json!({
        "hub": session_hub,
        "stream": subscription.stream,
        "tag": subscription.tag,
        "list": subscription.list,
    });
    let response = post_json_to_stream_hub(
        env,
        binding,
        channel_hub,
        STREAM_HUB_FORWARD_REGISTER_PATH,
        &body,
    )
    .await?;
    if response.status_code() >= 400 {
        return Err(worker::Error::RustError(format!(
            "stream hub forward registration failed with HTTP {}",
            response.status_code()
        )));
    }
    Ok(())
}
