use super::retry::{
    fetch_stream_hub_with_inactive_retry, fetch_stream_hub_with_inactive_retry_map,
};
use super::{
    STREAM_HUB_ACCOUNT_HEADER, STREAM_HUB_INTERNAL_HEADERS, STREAM_HUB_INTERNAL_ORIGIN,
    STREAM_HUB_LIST_HEADER, STREAM_HUB_STREAM_HEADER, STREAM_HUB_TAG_HEADER,
    STREAM_HUB_WEBSOCKET_PATH,
};
use worker::{Env, Method, Request, RequestInit, Response, Result, WebSocket};

/// Subscription the Worker authorised for a hub connection.
pub(crate) struct StreamHubUpgradeParams<'a> {
    pub(crate) stream: Option<&'a str>,
    pub(crate) tag: Option<&'a str>,
    pub(crate) list: Option<&'a str>,
    pub(crate) account_id: Option<&'a str>,
}

pub(super) fn set_stream_hub_subscription_headers(
    headers: &worker::Headers,
    params: &StreamHubUpgradeParams<'_>,
) -> Result<()> {
    if let Some(stream) = params
        .stream
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.set(STREAM_HUB_STREAM_HEADER, stream)?;
    }
    if let Some(tag) = params.tag.map(str::trim).filter(|value| !value.is_empty()) {
        headers.set(STREAM_HUB_TAG_HEADER, tag)?;
    }
    if let Some(list) = params.list.map(str::trim).filter(|value| !value.is_empty()) {
        headers.set(STREAM_HUB_LIST_HEADER, list)?;
    }
    if let Some(account_id) = params
        .account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.set(STREAM_HUB_ACCOUNT_HEADER, account_id)?;
    }
    Ok(())
}

pub(super) fn is_internal_stream_hub_header(name: &str) -> bool {
    STREAM_HUB_INTERNAL_HEADERS
        .iter()
        .any(|internal| internal.eq_ignore_ascii_case(name))
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
    let stream = stream.to_owned();
    let tag = tag.map(str::to_owned);
    let list = list.map(str::to_owned);
    let account_id = account_id.map(str::to_owned);

    fetch_stream_hub_with_inactive_retry_map(
        env,
        binding,
        hub_name,
        "connect",
        || {
            let headers = worker::Headers::new();
            headers.set("Upgrade", "websocket")?;
            set_stream_hub_subscription_headers(
                &headers,
                &StreamHubUpgradeParams {
                    stream: Some(stream.as_str()),
                    tag: tag.as_deref(),
                    list: list.as_deref(),
                    account_id: account_id.as_deref(),
                },
            )?;

            let mut init = RequestInit::new();
            init.with_method(Method::Get).with_headers(headers);
            let url = format!("{STREAM_HUB_INTERNAL_ORIGIN}{STREAM_HUB_WEBSOCKET_PATH}");
            Request::new_with_init(&url, &init)
        },
        |response| {
            let websocket = response.websocket().ok_or_else(|| {
                worker::Error::RustError(
                    "stream hub websocket upgrade did not return a socket".to_owned(),
                )
            })?;
            websocket.accept()?;
            Ok(websocket)
        },
    )
    .await
}

pub(crate) async fn upgrade_stream_hub_websocket(
    env: &Env,
    binding: &str,
    hub_name: &str,
    req: Request,
    params: &StreamHubUpgradeParams<'_>,
) -> Result<Response> {
    let query = req
        .url()?
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let url = format!("{STREAM_HUB_INTERNAL_ORIGIN}{STREAM_HUB_WEBSOCKET_PATH}{query}");
    let header_pairs: Vec<(String, String)> = req.headers().entries().collect();
    let stream = params.stream.map(str::to_owned);
    let tag = params.tag.map(str::to_owned);
    let list = params.list.map(str::to_owned);
    let account_id = params.account_id.map(str::to_owned);

    fetch_stream_hub_with_inactive_retry(env, binding, hub_name, "upgrade", || {
        let headers = worker::Headers::new();
        for (name, value) in &header_pairs {
            if is_internal_stream_hub_header(name) {
                continue;
            }
            headers.set(name, value)?;
        }
        set_stream_hub_subscription_headers(
            &headers,
            &StreamHubUpgradeParams {
                stream: stream.as_deref(),
                tag: tag.as_deref(),
                list: list.as_deref(),
                account_id: account_id.as_deref(),
            },
        )?;
        if !headers
            .get("Upgrade")?
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        {
            headers.set("Upgrade", "websocket")?;
        }

        let mut init = RequestInit::new();
        init.with_method(Method::Get).with_headers(headers);
        Request::new_with_init(&url, &init)
    })
    .await
}
