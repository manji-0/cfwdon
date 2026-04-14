use crate::{
    AppConfig, D1Database, LocalAccount, RemoteActorProfile, delete_follower_by_actor,
    handle_inbox_interaction_undo, handle_inbox_poll_vote_undo, is_follow_undo,
};
use worker::Result;

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
    if !is_follow_undo(activity.get("object"), actor_uri, &remote_actor.actor_uri) {
        if handle_inbox_poll_vote_undo(db, activity, remote_actor, account, config).await? {
            return Ok(());
        }
        return handle_inbox_interaction_undo(db, activity, remote_actor).await;
    }

    delete_follower_by_actor(db, &account.id, actor_uri, &remote_actor.actor_uri).await
}
