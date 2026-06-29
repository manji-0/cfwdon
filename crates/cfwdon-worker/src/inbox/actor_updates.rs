use super::{
    D1Database, LocalAccount, RemoteActorProfile, Result, activity_object_id,
    has_any_local_followers_for_remote_actor, is_activitypub_actor_type,
    is_local_account_following_remote_actor, parse_remote_actor_profile_document,
    upsert_remote_actor, validate_remote_actor_profile_urls,
};

pub(crate) async fn handle_inbox_actor_update(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: Option<&LocalAccount>,
) -> Result<()> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    if !is_activitypub_actor_type(object.get("type").and_then(serde_json::Value::as_str)) {
        return Ok(());
    }

    let object_actor_uri = activity_object_id(Some(object))
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
        .unwrap_or_default();
    if object_actor_uri != remote_actor.actor_uri {
        return Ok(());
    }

    let is_relevant = match account {
        Some(account) => {
            is_local_account_following_remote_actor(db, account.id(), &remote_actor.actor_uri)
                .await?
        }
        None => has_any_local_followers_for_remote_actor(db, &remote_actor.actor_uri).await?,
    };
    if !is_relevant {
        return Ok(());
    }

    let refreshed = parse_remote_actor_profile_document(object, &remote_actor.actor_uri)?;
    validate_remote_actor_profile_urls(&refreshed).await?;
    upsert_remote_actor(db, &refreshed).await
}
