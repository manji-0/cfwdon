use super::{
    AppConfig, FollowAccountRequest, LocalAccount, RemoteActorRow, actor_url,
    build_follow_activity, build_undo_follow_activity, count_followers_by_actor,
    delete_follow_by_target, find_active_mute, find_follow_by_target,
    has_pending_follow_request_from_account, has_pending_follow_request_from_actor,
    is_blocking_actor, load_account_social_metadata, load_follow_activity_id,
    queue_remote_actor_activity, queue_remote_actor_activity_required, remote_account_rest_id,
    upsert_remote_follow,
};
use js_sys::Date;
use serde::Serialize;
use worker::{D1Database, Error, Result};

#[derive(Debug, Serialize)]
pub(crate) struct RelationshipResponse {
    pub(crate) id: String,
    pub(crate) following: bool,
    pub(crate) showing_reblogs: bool,
    pub(crate) notifying: bool,
    pub(crate) languages: Option<Vec<String>>,
    pub(crate) followed_by: bool,
    pub(crate) blocking: bool,
    pub(crate) blocked_by: bool,
    pub(crate) muting: bool,
    pub(crate) muting_notifications: bool,
    pub(crate) muting_expires_at: Option<String>,
    pub(crate) requested: bool,
    pub(crate) requested_by: bool,
    pub(crate) domain_blocking: bool,
    pub(crate) endorsed: bool,
    pub(crate) note: String,
}

pub(crate) async fn build_relationship_for_target(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    target_id: &str,
    target_actor_uri: &str,
) -> Result<RelationshipResponse> {
    let follow = find_follow_by_target(db, &viewer.id, target_actor_uri).await?;
    let reciprocal =
        find_follow_by_target(db, target_id, &actor_url(config, &viewer.username)).await?;
    let languages = follow
        .as_ref()
        .and_then(|row| row.languages_json.as_deref())
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok());
    let state = follow
        .as_ref()
        .map(|row| row.state.as_str())
        .unwrap_or("none");
    let followed_by_remote = count_followers_by_actor(db, &viewer.id, target_actor_uri).await? > 0;
    let blocking = is_blocking_actor(db, &viewer.id, target_actor_uri).await?;
    let blocked_by = if target_id.starts_with("r_") {
        false
    } else {
        is_blocking_actor(db, target_id, &actor_url(config, &viewer.username)).await?
    };
    let mute = find_active_mute(db, &viewer.id, target_actor_uri).await?;
    let requested_by = if target_id.starts_with("r_") {
        has_pending_follow_request_from_actor(db, &viewer.id, target_actor_uri).await?
    } else {
        has_pending_follow_request_from_account(db, &viewer.id, target_id).await?
    };
    let social_metadata = load_account_social_metadata(db, &viewer.id, target_actor_uri).await?;

    Ok(RelationshipResponse {
        id: target_id.to_owned(),
        following: state == "accepted",
        showing_reblogs: follow
            .as_ref()
            .map(|row| row.show_reblogs != 0)
            .unwrap_or(false),
        notifying: follow.as_ref().map(|row| row.notify != 0).unwrap_or(false),
        languages,
        followed_by: reciprocal
            .as_ref()
            .map(|row| row.state == "accepted")
            .unwrap_or(false)
            || followed_by_remote,
        blocking,
        blocked_by,
        muting: mute.is_some(),
        muting_notifications: mute
            .as_ref()
            .map(|row| row.notifications != 0)
            .unwrap_or(false),
        muting_expires_at: mute.and_then(|row| row.expires_at),
        requested: state == "pending",
        requested_by,
        domain_blocking: false,
        endorsed: social_metadata
            .as_ref()
            .map(|row| row.endorsed != 0)
            .unwrap_or(false),
        note: social_metadata.map(|row| row.note).unwrap_or_default(),
    })
}

pub(crate) fn expiry_from_duration_seconds(duration: u32) -> Result<String> {
    let now = Date::new_0();
    now.set_time(now.get_time() + (duration as f64 * 1000.0));
    now.to_iso_string()
        .as_string()
        .ok_or_else(|| Error::RustError("failed to compute mute expiry timestamp".to_owned()))
}

pub(crate) async fn follow_remote_account(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
    request: &FollowAccountRequest,
) -> Result<RelationshipResponse> {
    let (_, payload) = build_follow_activity(config, follower, &actor.actor_uri)?;
    let follow_activity_id =
        queue_remote_actor_activity_required(db, &follower.id, &actor.actor_uri, &payload).await?;
    upsert_remote_follow(db, follower, actor, request, &follow_activity_id).await?;
    build_relationship_for_target(
        db,
        config,
        follower,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}

pub(crate) async fn unfollow_remote_account(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
) -> Result<RelationshipResponse> {
    if let Some(follow_activity_id) =
        load_follow_activity_id(db, &follower.id, &actor.actor_uri).await?
    {
        let payload =
            build_undo_follow_activity(config, follower, &follow_activity_id, &actor.actor_uri)?;
        let _ = queue_remote_actor_activity(db, &follower.id, &actor.actor_uri, &payload).await?;
    }

    delete_follow_by_target(db, &follower.id, &actor.actor_uri).await?;
    build_relationship_for_target(
        db,
        config,
        follower,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}
