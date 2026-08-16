use crate::D1Database;
#[allow(unused_imports)]
pub(crate) use crate::*;

mod expiration_store;
mod request_parsing;
mod vote_route;
pub(crate) use expiration_store::*;
pub(crate) use request_parsing::*;
pub(crate) use vote_route::vote_in_poll;

use super::auth::{
    extract_authenticated_user, find_account_by_id, find_authenticated_local_account,
};
use super::enqueue_status_update_activity;
use super::runtime_config::load_config;
use super::time_html::is_iso_timestamp_in_past;
use super::{
    apply_poll_vote, apply_remote_poll_vote, build_mastodon_poll_response,
    build_remote_mastodon_poll_response, can_view_local_status, find_remote_actor_by_actor_uri,
    find_remote_status_by_id, find_remote_status_poll_by_id, find_remote_status_poll_by_status_id,
    find_status_by_id, find_status_poll_by_id, refresh_remote_poll_if_needed,
    remote_poll_is_visible_to_viewer, send_poll_end_notifications,
};
use cfwdon_core::AppConfig;
use serde::{Deserialize, Serialize};
use worker::{Env, Error, Request, Response, Result, RouteContext};

#[derive(Debug, Default, Serialize)]
pub(crate) struct PollExpirationProcessResponse {
    queued: u32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExpiredPollQueueRow {
    pub(crate) poll_id: String,
    pub(crate) status_id: String,
    pub(crate) account_id: String,
}

pub(crate) async fn poll_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let poll_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing poll id route parameter".to_owned()))?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    if let Some(poll) = find_status_poll_by_id(&db, &poll_id).await? {
        let Some(status) = find_status_by_id(&db, &poll.status_id).await? else {
            return Response::error("poll not found", 404);
        };
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("poll not found", 404);
        };
        if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
            return Response::error("poll not found", 404);
        }

        return Response::from_json(
            &build_mastodon_poll_response(&db, &poll, viewer.as_ref())
                .await?
                .ok_or_else(|| Error::RustError("poll not found".to_owned()))?,
        );
    }

    let Some(mut poll) = find_remote_status_poll_by_id(&db, &poll_id).await? else {
        return Response::error("poll not found", 404);
    };
    let Some(mut status) = find_remote_status_by_id(&db, &poll.status_id).await? else {
        return Response::error("poll not found", 404);
    };
    refresh_remote_poll_if_needed(&db, &config, &status, &poll, viewer.as_ref()).await?;
    if let Some(refreshed_status) = find_remote_status_by_id(&db, &poll.status_id).await? {
        status = refreshed_status;
    }
    if let Some(refreshed_poll) = find_remote_status_poll_by_status_id(&db, &status.id).await? {
        poll = refreshed_poll;
    }

    if !remote_poll_is_visible_to_viewer(&db, &config, &poll, &status, viewer.as_ref()).await? {
        return Response::error("poll not found", 404);
    }

    Response::from_json(
        &build_remote_mastodon_poll_response(&db, &poll, viewer.as_ref())
            .await?
            .ok_or_else(|| Error::RustError("poll not found".to_owned()))?,
    )
}

pub(crate) async fn process_expired_polls_for_config(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
) -> Result<PollExpirationProcessResponse> {
    let mut summary = PollExpirationProcessResponse::default();

    for row in list_expired_polls_requiring_federation_close(db, 64).await? {
        let Some(status) = find_status_by_id(db, &row.status_id).await? else {
            continue;
        };
        let Some(account) = find_account_by_id(db, &row.account_id).await? else {
            continue;
        };
        if enqueue_status_update_activity(db, config, &account, &status)
            .await
            .is_ok()
        {
            let _ = send_poll_end_notifications(
                db,
                config,
                env,
                &row.poll_id,
                &row.status_id,
                &row.account_id,
            )
            .await;
            mark_status_poll_federated_closed(db, &row.poll_id).await?;
            summary.queued += 1;
        }
    }

    Ok(summary)
}

pub(crate) async fn process_expired_polls(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {}
        None => return Response::error("Auth0 authentication required", 401),
    }

    let db = crate::bind_request_d1(&ctx, &config)?;
    let summary = process_expired_polls_for_config(&db, &config, Some(&ctx.env)).await?;
    Response::from_json(&summary)
}
