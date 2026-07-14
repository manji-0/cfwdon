use super::{
    AppConfig, D1Database, LocalAccount, RemoteActorProfile, Result, actor_url,
    build_accept_activity, delete_remote_follow_request_by_actor, follow_targets_local_actor,
    handle_inbox_collection_feature_accept, handle_inbox_collection_feature_reject,
    queue_remote_actor_activity_required, update_follow_state_from_response, upsert_follower,
    upsert_remote_follow_request,
};
use cfwdon_domain::FollowInboxResponse;

pub(crate) async fn handle_inbox_follow(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    if !follow_targets_local_actor(
        activity.get("object"),
        &actor_url(config, account.username()),
    ) {
        return Ok(());
    }

    let locked = account.is_locked();
    let follow_activity_id = activity.get("id").and_then(serde_json::Value::as_str);

    if locked {
        upsert_remote_follow_request(db, account.id(), remote_actor, follow_activity_id).await?;
        return Ok(());
    }

    delete_remote_follow_request_by_actor(
        db,
        account.id(),
        &remote_actor.actor_uri,
        &remote_actor.actor_uri,
    )
    .await?;
    upsert_follower(db, account.id(), remote_actor, follow_activity_id).await?;

    let accept_activity =
        build_accept_activity(config, account, activity, &remote_actor.actor_uri)?;
    let _ = queue_remote_actor_activity_required(
        db,
        account.id(),
        &remote_actor.actor_uri,
        &accept_activity,
    )
    .await;

    Ok(())
}

pub(crate) async fn handle_inbox_accept(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    if handle_inbox_collection_feature_accept(db, config, activity, remote_actor).await? {
        return Ok(());
    }
    update_follow_state_from_response(
        db,
        activity,
        remote_actor,
        FollowInboxResponse::Accept.as_str(),
    )
    .await
}

pub(crate) async fn handle_inbox_reject(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    if handle_inbox_collection_feature_reject(db, activity, remote_actor).await? {
        return Ok(());
    }
    update_follow_state_from_response(
        db,
        activity,
        remote_actor,
        FollowInboxResponse::Reject.as_str(),
    )
    .await
}
