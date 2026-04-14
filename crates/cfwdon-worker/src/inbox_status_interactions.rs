use crate::{
    AppConfig, D1Database, LocalAccount, RemoteActorProfile, activity_object_id,
    delete_remote_favourite, delete_remote_reblog, find_local_status_by_object_uri,
};
use worker::Result;

pub(crate) async fn handle_inbox_like(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<()> {
    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };
    let Some(status) = find_local_status_by_object_uri(db, config, object_uri).await? else {
        return Ok(());
    };
    if status.account_id != account.id {
        return Ok(());
    }
    let activity_uri = activity.get("id").and_then(serde_json::Value::as_str);
    crate::upsert_remote_favourite(
        db,
        &remote_actor.actor_uri,
        &status.id,
        object_uri,
        activity_uri,
    )
    .await
}

pub(crate) async fn handle_inbox_announce(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<()> {
    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };
    let Some(status) = find_local_status_by_object_uri(db, config, object_uri).await? else {
        return Ok(());
    };
    if status.account_id != account.id {
        return Ok(());
    }
    let activity_uri = activity.get("id").and_then(serde_json::Value::as_str);
    crate::upsert_remote_reblog(
        db,
        &remote_actor.actor_uri,
        &status.id,
        object_uri,
        activity_uri,
    )
    .await
}

pub(crate) async fn handle_inbox_interaction_undo(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(object) = activity.get("object") else {
        return Ok(());
    };
    let activity_type = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let target_uri = object
        .get("object")
        .and_then(|value| activity_object_id(Some(value)))
        .unwrap_or_default();
    let activity_uri = object.get("id").and_then(serde_json::Value::as_str);

    match activity_type {
        "Like" => {
            delete_remote_favourite(db, &remote_actor.actor_uri, target_uri, activity_uri).await?
        }
        "Announce" => {
            delete_remote_reblog(db, &remote_actor.actor_uri, target_uri, activity_uri).await?
        }
        _ => {}
    }

    Ok(())
}
