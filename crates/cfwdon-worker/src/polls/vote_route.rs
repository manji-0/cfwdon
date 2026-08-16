use super::{
    apply_poll_vote, apply_remote_poll_vote, build_mastodon_poll_response,
    build_remote_mastodon_poll_response, can_view_local_status, enqueue_status_update_activity,
    find_authenticated_local_account, find_remote_actor_by_actor_uri, find_remote_status_by_id,
    find_remote_status_poll_by_id, find_remote_status_poll_by_status_id, find_status_by_id,
    find_status_poll_by_id, is_iso_timestamp_in_past, load_config, parse_poll_vote_request,
    refresh_remote_poll_if_needed, remote_poll_is_visible_to_viewer,
};
use crate::auth::find_account_by_id;
use worker::{Error, Request, Response, Result, RouteContext};

pub(crate) async fn vote_in_poll(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let poll_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing poll id route parameter".to_owned()))?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let choices = match parse_poll_vote_request(req).await {
        Ok(choices) => choices,
        Err(message) => return Response::error(message, 422),
    };
    if let Some(poll) = find_status_poll_by_id(&db, &poll_id).await? {
        let Some(status) = find_status_by_id(&db, &poll.status_id).await? else {
            return Response::error("poll not found", 404);
        };
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("poll not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("poll not found", 404);
        }
        if is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false) {
            return Response::error("poll has already expired", 422);
        }

        if let Err(error) = apply_poll_vote(&db, &poll, viewer.id(), &choices).await {
            return match error {
                Error::RustError(message) => Response::error(message, 422),
                other => Err(other),
            };
        }
        let _ = enqueue_status_update_activity(&db, &config, &account, &status).await;
        return Response::from_json(
            &build_mastodon_poll_response(&db, &poll, Some(&viewer))
                .await?
                .ok_or_else(|| Error::RustError("poll not found".to_owned()))?,
        );
    }

    if let Some(mut poll) = find_remote_status_poll_by_id(&db, &poll_id).await? {
        let Some(mut status) = find_remote_status_by_id(&db, &poll.status_id).await? else {
            return Response::error("poll not found", 404);
        };
        refresh_remote_poll_if_needed(&db, &config, &status, &poll, Some(&viewer)).await?;
        if let Some(refreshed_status) = find_remote_status_by_id(&db, &poll.status_id).await? {
            status = refreshed_status;
        }
        if let Some(refreshed_poll) = find_remote_status_poll_by_status_id(&db, &status.id).await? {
            poll = refreshed_poll;
        }
        if !remote_poll_is_visible_to_viewer(&db, &config, &poll, &status, Some(&viewer)).await? {
            return Response::error("poll not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("poll not found", 404);
        };
        if poll.expired != 0
            || poll
                .expires_at
                .as_deref()
                .map(|value| is_iso_timestamp_in_past(value).unwrap_or(false))
                .unwrap_or(false)
        {
            return Response::error("poll has already expired", 422);
        }

        if let Err(error) =
            apply_remote_poll_vote(&db, &config, &viewer, &actor, &status, &poll, &choices).await
        {
            return match error {
                Error::RustError(message) => Response::error(message, 422),
                other => Err(other),
            };
        }
        if let Some(refreshed_poll) = find_remote_status_poll_by_status_id(&db, &status.id).await? {
            poll = refreshed_poll;
        }
        return Response::from_json(
            &build_remote_mastodon_poll_response(&db, &poll, Some(&viewer))
                .await?
                .ok_or_else(|| Error::RustError("poll not found".to_owned()))?,
        );
    }

    Response::error("poll not found", 404)
}
