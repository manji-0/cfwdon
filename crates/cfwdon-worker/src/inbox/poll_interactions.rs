use super::{
    AppConfig, LocalAccount, RemoteActorProfile, activity_object_id, apply_incoming_poll_vote,
    delete_incoming_poll_vote, enqueue_status_update_activity, find_local_status_by_object_uri,
    find_status_by_id, find_status_poll_by_status_id,
    find_status_poll_vote_for_remote_actor_by_activity_uri, is_iso_timestamp_in_past,
    list_status_poll_options,
};
use worker::Result;

use crate::D1Database;
pub(crate) async fn handle_inbox_poll_vote_undo(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<bool> {
    let Some(undo_target) = activity.get("object") else {
        return Ok(false);
    };
    let activity_uri = activity_object_id(Some(undo_target)).map(str::to_owned);
    let nested_object = undo_target.get("object").filter(|value| value.is_object());
    let choice_name = nested_object
        .and_then(|object| object.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let in_reply_to = nested_object
        .and_then(|object| object.get("inReplyTo"))
        .and_then(serde_json::Value::as_str);

    let (poll, status) = if let Some(activity_uri) = activity_uri.as_deref()
        && let Some(vote) = find_status_poll_vote_for_remote_actor_by_activity_uri(
            db,
            &remote_actor.actor_uri,
            activity_uri,
        )
        .await?
    {
        let Some(status) = find_status_by_id(db, &vote.status_id).await? else {
            return Ok(false);
        };
        let Some(poll) = find_status_poll_by_status_id(db, &vote.status_id).await? else {
            return Ok(false);
        };
        (poll, status)
    } else if let Some(in_reply_to) = in_reply_to
        && let Some(status) = find_local_status_by_object_uri(db, config, in_reply_to).await?
    {
        let Some(poll) = find_status_poll_by_status_id(db, &status.id).await? else {
            return Ok(false);
        };
        (poll, status)
    } else {
        return Ok(false);
    };

    if status.account_id != account.id() {
        return Ok(false);
    }

    let deleted = delete_incoming_poll_vote(
        db,
        &poll,
        &remote_actor.actor_uri,
        activity_uri.as_deref(),
        choice_name,
    )
    .await?;
    if deleted {
        let _ = enqueue_status_update_activity(db, config, account, &status).await;
    }
    Ok(deleted)
}

pub(crate) async fn handle_inbox_poll_vote(
    db: &D1Database,
    object: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
    activity_uri: Option<&str>,
) -> Result<bool> {
    let Some(in_reply_to) = object.get("inReplyTo").and_then(serde_json::Value::as_str) else {
        return Ok(false);
    };
    let Some(choice_name) = object.get("name").and_then(serde_json::Value::as_str) else {
        return Ok(false);
    };
    let Some(status) = find_local_status_by_object_uri(db, config, in_reply_to).await? else {
        return Ok(false);
    };
    if status.account_id != account.id() {
        return Ok(false);
    }
    if !crate::remote_actor_may_interact_with_local_status(db, &status, &remote_actor.actor_uri)
        .await?
    {
        return Ok(false);
    }
    let Some(poll) = find_status_poll_by_status_id(db, &status.id).await? else {
        return Ok(false);
    };
    if is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false) {
        return Ok(true);
    }
    let options = list_status_poll_options(db, &poll.id).await?;
    let Some(choice) = options
        .iter()
        .position(|option| option.title == choice_name.trim())
    else {
        return Ok(true);
    };

    apply_incoming_poll_vote(
        db,
        &poll,
        &remote_actor.actor_uri,
        choice as u32,
        activity_uri,
    )
    .await?;
    let _ = enqueue_status_update_activity(db, config, account, &status).await;
    Ok(true)
}
