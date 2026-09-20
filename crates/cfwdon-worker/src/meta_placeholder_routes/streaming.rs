mod announcements;
mod auth;
mod batches;
mod budget;
mod channels;
mod hub_routing;
mod poll;
mod sse;
mod status_deltas;
mod websocket;

use super::invalid_access_token_response;
use crate::{Request, Response, Result, RouteContext, load_config};
use auth::{StreamingAuthOutcome, resolve_streaming_auth};
use channels::{StreamingQuery, streaming_bad_request_response, websocket_protocol_access_token};
use sse::streaming_sse_response;
use websocket::streaming_websocket_upgrade_response;

pub(crate) use channels::{
    StreamingChannelValidationError, streaming_channel_requires_auth,
    validate_streaming_channel_request,
};

const STREAMING_POLL_INTERVAL_SECS: u64 = 3;

pub(crate) async fn streaming_placeholder_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let query: StreamingQuery = req.query().unwrap_or_default();
    let wants_websocket = req
        .headers()
        .get("Upgrade")
        .ok()
        .flatten()
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let websocket_protocol_token = if wants_websocket {
        websocket_protocol_access_token(&req)?
    } else {
        None
    };
    let extra_path = ctx
        .param("any")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let initial_stream = if wants_websocket && query.stream.is_none() && extra_path.is_none() {
        None
    } else {
        match validate_streaming_channel_request(
            query.stream.as_deref(),
            query.tag.as_deref(),
            query.list.as_deref(),
            extra_path,
        ) {
            Ok(stream) => Some(stream),
            Err(error) => return streaming_bad_request_response(error),
        }
    };
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let authenticated = match resolve_streaming_auth(
        &req,
        &db,
        &config,
        query.access_token.as_deref(),
        websocket_protocol_token.as_deref(),
    )
    .await?
    {
        StreamingAuthOutcome::InvalidToken => return invalid_access_token_response(),
        StreamingAuthOutcome::Viewer(viewer) => viewer,
    };

    if initial_stream
        .as_deref()
        .is_some_and(streaming_channel_requires_auth)
        && authenticated.is_none()
    {
        return invalid_access_token_response();
    }

    if wants_websocket {
        return streaming_websocket_upgrade_response(
            &ctx.env,
            req,
            db,
            config,
            initial_stream,
            query.tag.clone(),
            query.list.clone(),
            authenticated,
            websocket_protocol_token.as_deref(),
        )
        .await;
    }

    let Some(stream) = initial_stream else {
        return streaming_bad_request_response(
            StreamingChannelValidationError::UnknownChannelRequested,
        );
    };

    streaming_sse_response(
        &ctx.env,
        db,
        config,
        stream,
        query.tag,
        query.list,
        authenticated,
    )
}
