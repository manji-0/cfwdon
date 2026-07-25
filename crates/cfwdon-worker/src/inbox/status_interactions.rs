use super::{
    AppConfig, D1Database, LocalAccount, RemoteActorProfile, StatusRow, activity_object_id,
    delete_remote_favourite, delete_remote_reblog, delete_remote_status_by_object_uri,
    extract_remote_note_object, find_conversation_id_by_status_id, find_local_status_by_object_uri,
    is_blocking_actor, is_remote_actor_following_local_account, list_conversation_participants,
    object_attributed_to_remote_actor, upsert_remote_actor, upsert_remote_reblog,
    upsert_remote_reblog_status, upsert_remote_status,
};
use worker::Result;

pub(crate) async fn remote_actor_may_interact_with_local_status(
    db: &D1Database,
    status: &StatusRow,
    remote_actor_uri: &str,
) -> Result<bool> {
    if is_blocking_actor(db, &status.account_id, remote_actor_uri).await? {
        return Ok(false);
    }

    match status.visibility.as_str() {
        "public" | "unlisted" => Ok(true),
        "private" => {
            is_remote_actor_following_local_account(db, &status.account_id, remote_actor_uri).await
        }
        "direct" => {
            let Some(conversation_id) = find_conversation_id_by_status_id(db, &status.id).await?
            else {
                return Ok(false);
            };
            let participants = list_conversation_participants(db, &conversation_id).await?;
            Ok(participants.iter().any(|participant| {
                participant == remote_actor_uri
                    || participant.eq_ignore_ascii_case(remote_actor_uri)
            }))
        }
        _ => Ok(false),
    }
}

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
    if status.account_id != account.id() {
        return Ok(());
    }
    if !remote_actor_may_interact_with_local_status(db, &status, &remote_actor.actor_uri).await? {
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
    let mut actor_upserted = false;
    if let Some(object) = extract_remote_note_object(activity).filter(|object| {
        object_attributed_to_remote_actor(object, activity, &remote_actor.actor_uri)
    }) {
        upsert_remote_actor(db, remote_actor).await?;
        actor_upserted = true;
        upsert_remote_status(db, config, remote_actor, object).await?;
    }

    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };
    if !actor_upserted {
        upsert_remote_actor(db, remote_actor).await?;
    }
    upsert_remote_reblog_status(db, config, remote_actor, activity).await?;

    if let Some(status) = find_local_status_by_object_uri(db, config, object_uri).await? {
        if status.account_id != account.id() {
            return Ok(());
        }
        if !remote_actor_may_interact_with_local_status(db, &status, &remote_actor.actor_uri)
            .await?
        {
            return Ok(());
        }
        let activity_uri = activity.get("id").and_then(serde_json::Value::as_str);
        upsert_remote_reblog(
            db,
            &remote_actor.actor_uri,
            &status.id,
            object_uri,
            activity_uri,
        )
        .await?;
    }

    Ok(())
}

pub(crate) async fn handle_inbox_interaction_undo(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(object) = activity.get("object") else {
        return Ok(());
    };
    let activity_type = crate::activitypub_primary_type(object).unwrap_or_default();
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
            delete_remote_reblog(db, &remote_actor.actor_uri, target_uri, activity_uri).await?;
            if let Some(announce_activity_id) = object.get("id").and_then(serde_json::Value::as_str)
            {
                delete_remote_status_by_object_uri(db, announce_activity_id).await?;
            }
        }
        _ => {}
    }

    Ok(())
}
