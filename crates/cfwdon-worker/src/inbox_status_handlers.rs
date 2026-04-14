use crate::{
    AppConfig, D1Database, LocalAccount, RemoteActorProfile, Result, activity_object_id,
    delete_remote_status_by_id, find_remote_status_by_object_uri, handle_inbox_actor_update,
    handle_inbox_poll_vote, is_activitypub_actor_type, is_supported_remote_status_object_type,
    note_targets_account_or_followers, upsert_remote_actor, upsert_remote_status,
};

pub(crate) async fn handle_inbox_create(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<()> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    if !is_supported_remote_status_object_type(
        object.get("type").and_then(serde_json::Value::as_str),
    ) {
        return Ok(());
    }

    let attributed_to = object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
        .unwrap_or_default();
    if attributed_to != remote_actor.actor_uri {
        return Ok(());
    }
    if handle_inbox_poll_vote(
        db,
        object,
        remote_actor,
        account,
        config,
        activity.get("id").and_then(serde_json::Value::as_str),
    )
    .await?
    {
        return Ok(());
    }
    if !note_targets_account_or_followers(object, account, config) {
        return Ok(());
    }

    upsert_remote_actor(db, remote_actor).await?;
    upsert_remote_status(db, remote_actor, object).await
}

pub(crate) async fn handle_inbox_update(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<()> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    if is_activitypub_actor_type(object.get("type").and_then(serde_json::Value::as_str)) {
        return handle_inbox_actor_update(db, activity, remote_actor, Some(account)).await;
    }
    if !is_supported_remote_status_object_type(
        object.get("type").and_then(serde_json::Value::as_str),
    ) {
        return Ok(());
    }

    let attributed_to = object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
        .unwrap_or_default();
    if attributed_to != remote_actor.actor_uri {
        return Ok(());
    }
    if !note_targets_account_or_followers(object, account, config) {
        return Ok(());
    }

    upsert_remote_actor(db, remote_actor).await?;
    upsert_remote_status(db, remote_actor, object).await
}

pub(crate) async fn handle_inbox_delete(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };
    let Some(status) = find_remote_status_by_object_uri(db, object_uri).await? else {
        return Ok(());
    };
    if status.actor_uri != remote_actor.actor_uri {
        return Ok(());
    }

    delete_remote_status_by_id(db, &status.id).await
}
