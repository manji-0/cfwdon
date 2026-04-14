use crate::{
    AppConfig, D1Database, LocalAccount, RemoteActorProfile, Result, actor_url,
    build_accept_activity, follow_targets_local_actor, queue_remote_actor_activity_required,
    update_follow_state_from_response, upsert_follower,
};

pub(crate) async fn handle_inbox_follow(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    if !follow_targets_local_actor(
        activity.get("object"),
        &actor_url(config, &account.username),
    ) {
        return Ok(());
    }

    upsert_follower(db, &account.id, remote_actor).await?;

    let accept_activity =
        build_accept_activity(config, account, activity, &remote_actor.actor_uri)?;
    let _ = queue_remote_actor_activity_required(
        db,
        &account.id,
        &remote_actor.actor_uri,
        &accept_activity,
    )
    .await;

    Ok(())
}

pub(crate) async fn handle_inbox_accept(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    update_follow_state_from_response(db, activity, remote_actor, "accepted").await
}

pub(crate) async fn handle_inbox_reject(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    update_follow_state_from_response(db, activity, remote_actor, "rejected").await
}
