use super::{
    AppConfig, RemoteActorProfile, Result, VerifiedActivityPubDelivery, activitypub_has_type,
    fetch_remote_actor_profile, handle_inbox_delete, mark_federation_relay_accepted,
    mark_federation_relay_rejected, note_targets_public, object_attributed_to_remote_actor,
    object_has_supported_remote_status_type, relay_delivery_is_enabled,
    relay_follow_activity_id_from_accept, upsert_remote_actor, upsert_remote_status,
};
use worker::Env;

use crate::D1Database;

pub(crate) async fn handle_relay_delivered_activity(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
    delivery: &VerifiedActivityPubDelivery,
    env: Option<&Env>,
) -> Result<bool> {
    if !delivery.relayed {
        return Ok(false);
    }
    if !relay_delivery_is_enabled(db, &delivery.delivery_actor).await? {
        return Ok(false);
    }

    if activitypub_has_type(activity, "Accept") {
        if let Some(follow_activity_id) = relay_follow_activity_id_from_accept(activity) {
            let _ = mark_federation_relay_accepted(
                db,
                &follow_activity_id,
                &delivery.delivery_actor.actor_uri,
            )
            .await?;
        }
        return Ok(true);
    }

    if activitypub_has_type(activity, "Reject") {
        if let Some(follow_activity_id) = relay_follow_activity_id_from_accept(activity) {
            let _ = mark_federation_relay_rejected(db, &follow_activity_id).await?;
        }
        return Ok(true);
    }

    let content_actor = load_relay_content_actor(db, &delivery.content_actor_uri).await?;
    if activitypub_has_type(activity, "Create") {
        return handle_relay_create(db, config, activity, &content_actor, env).await;
    }
    if activitypub_has_type(activity, "Update") {
        return handle_relay_update(db, config, activity, &content_actor, env).await;
    }
    if activitypub_has_type(activity, "Delete") {
        handle_inbox_delete(db, config, activity, &content_actor, env).await?;
        return Ok(true);
    }

    Ok(true)
}

async fn load_relay_content_actor(db: &D1Database, actor_uri: &str) -> Result<RemoteActorProfile> {
    if let Some(actor) = crate::find_cached_remote_actor_profile_by_actor_uri(db, actor_uri).await?
    {
        return Ok(actor);
    }
    let actor = fetch_remote_actor_profile(actor_uri).await?;
    upsert_remote_actor(db, &actor).await?;
    Ok(actor)
}

async fn handle_relay_create(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
    content_actor: &RemoteActorProfile,
    env: Option<&Env>,
) -> Result<bool> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(true);
    };
    if !object_has_supported_remote_status_type(object) {
        return Ok(true);
    }
    if !object_attributed_to_remote_actor(object, activity, &content_actor.actor_uri) {
        return Err(worker::Error::RustError(
            "activitypub unauthorized: relay object attribution mismatch".to_owned(),
        ));
    }
    if !note_targets_public(object) {
        return Ok(true);
    }
    upsert_remote_actor(db, content_actor).await?;
    upsert_remote_status(db, config, content_actor, object, env).await?;
    Ok(true)
}

async fn handle_relay_update(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
    content_actor: &RemoteActorProfile,
    env: Option<&Env>,
) -> Result<bool> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(true);
    };
    if !object_has_supported_remote_status_type(object) {
        return Ok(true);
    }
    if !object_attributed_to_remote_actor(object, activity, &content_actor.actor_uri) {
        return Err(worker::Error::RustError(
            "activitypub unauthorized: relay object attribution mismatch".to_owned(),
        ));
    }
    if !note_targets_public(object) {
        return Ok(true);
    }
    upsert_remote_actor(db, content_actor).await?;
    upsert_remote_status(db, config, content_actor, object, env).await?;
    Ok(true)
}
