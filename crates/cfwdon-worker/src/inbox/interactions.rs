use super::{
    AppConfig, D1Database, LocalAccount, RemoteActorProfile, actor_url, delete_follower_by_actor,
    delete_remote_follow_request_by_actor, find_follower_follow_activity_id,
    find_pending_remote_follow_request_by_actor, follow_targets_local_actor,
    handle_inbox_interaction_undo, handle_inbox_poll_vote_undo, is_follow_undo,
};
use worker::Result;

async fn string_undo_matches_known_follow(
    db: &D1Database,
    account: &LocalAccount,
    object_id: &str,
    actor_uri: &str,
    canonical_actor_uri: &str,
) -> Result<bool> {
    if let Some(follow_activity_id) =
        find_follower_follow_activity_id(db, &account.id, actor_uri, canonical_actor_uri).await?
        && follow_activity_id == object_id
    {
        return Ok(true);
    }

    for requester_actor_uri in [actor_uri, canonical_actor_uri] {
        let Some(request) =
            find_pending_remote_follow_request_by_actor(db, &account.id, requester_actor_uri)
                .await?
        else {
            continue;
        };
        if request.follow_activity_id.as_deref() == Some(object_id) {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn activity_is_follow_undo(
    db: &D1Database,
    account: &LocalAccount,
    activity: &serde_json::Value,
    actor_uri: &str,
    remote_actor: &RemoteActorProfile,
    config: &AppConfig,
) -> Result<bool> {
    let Some(object) = activity.get("object") else {
        return Ok(false);
    };

    if let Some(object_id) = object.as_str() {
        return string_undo_matches_known_follow(
            db,
            account,
            object_id,
            actor_uri,
            &remote_actor.actor_uri,
        )
        .await;
    }

    let local_actor_uri = actor_url(config, &account.username);
    Ok(
        is_follow_undo(Some(object), actor_uri, &remote_actor.actor_uri)
            && follow_targets_local_actor(object.get("object"), &local_actor_uri),
    )
}

pub(crate) async fn handle_inbox_undo(
    db: &D1Database,
    account: &LocalAccount,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    config: &AppConfig,
) -> Result<()> {
    let Some(actor_uri) = activity.get("actor").and_then(serde_json::Value::as_str) else {
        return Ok(());
    };
    if !activity_is_follow_undo(db, account, activity, actor_uri, remote_actor, config).await? {
        if handle_inbox_poll_vote_undo(db, activity, remote_actor, account, config).await? {
            return Ok(());
        }
        return handle_inbox_interaction_undo(db, activity, remote_actor).await;
    }

    delete_follower_by_actor(db, &account.id, actor_uri, &remote_actor.actor_uri).await?;
    delete_remote_follow_request_by_actor(db, &account.id, actor_uri, &remote_actor.actor_uri).await
}
