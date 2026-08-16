use super::{
    activitypub_value_id, collection_item_by_feature_request_uri, collection_row_by_id,
    delete_remote_collection_by_uri, delete_remote_collection_item_by_object,
    enqueue_collection_item_add_activity, is_remote_actor_collections_target,
    local_collection_id_from_uri, remote_account_rest_id, remote_collection_row_by_uri,
    update_collection_item_feature_state, upsert_remote_actor,
    upsert_remote_collection_from_object, upsert_remote_collection_item_from_object,
};
use crate::{RemoteActorProfile, Result, find_account_by_id};
use worker::d1::D1Type;

pub(crate) async fn handle_inbox_collection_add(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(target) = activitypub_value_id(activity.get("target")) else {
        return Ok(());
    };
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    upsert_remote_actor(db, remote_actor).await?;
    if is_remote_actor_collections_target(&remote_actor.actor_uri, target) {
        let _ = upsert_remote_collection_from_object(db, config, remote_actor, object).await?;
        return Ok(());
    }
    let Some(collection) = remote_collection_row_by_uri(db, target).await? else {
        return Ok(());
    };
    if collection.actor_uri != remote_actor.actor_uri {
        return Ok(());
    }
    upsert_remote_collection_item_from_object(db, config, &collection.id, &collection.uri, object)
        .await
}

pub(crate) async fn handle_inbox_collection_update(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<bool> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(false);
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("FeaturedCollection") {
        return Ok(false);
    }
    upsert_remote_actor(db, remote_actor).await?;
    let _ = upsert_remote_collection_from_object(db, config, remote_actor, object).await?;
    Ok(true)
}

pub(crate) async fn handle_inbox_collection_remove(
    db: &crate::D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(target) = activitypub_value_id(activity.get("target")) else {
        return Ok(());
    };
    let Some(object) = activity.get("object") else {
        return Ok(());
    };
    if is_remote_actor_collections_target(&remote_actor.actor_uri, target) {
        if let Some(collection_uri) = activitypub_value_id(Some(object)) {
            delete_remote_collection_by_uri(db, &remote_actor.actor_uri, collection_uri).await?;
        }
        return Ok(());
    }
    let Some(collection) = remote_collection_row_by_uri(db, target).await? else {
        return Ok(());
    };
    if collection.actor_uri != remote_actor.actor_uri {
        return Ok(());
    }
    delete_remote_collection_item_by_object(db, &collection.id, object).await
}

fn feature_response_object_uri(activity: &serde_json::Value) -> Option<&str> {
    activitypub_value_id(activity.get("object"))
}

fn feature_response_result_uri(activity: &serde_json::Value) -> Option<&str> {
    match activity.get("result") {
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .find_map(|value| activitypub_value_id(Some(value))),
        value => activitypub_value_id(value),
    }
}

pub(crate) async fn handle_inbox_collection_feature_accept(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<bool> {
    let Some(feature_request_uri) = feature_response_object_uri(activity) else {
        return Ok(false);
    };
    let Some(approval_uri) = feature_response_result_uri(activity) else {
        return Ok(false);
    };
    let Some((collection, item)) =
        collection_item_by_feature_request_uri(db, feature_request_uri).await?
    else {
        return Ok(false);
    };
    if item.target_account_ref != remote_account_rest_id(&remote_actor.actor_uri) {
        return Ok(false);
    }
    let Some(owner) = find_account_by_id(db, &collection.account_id).await? else {
        return Ok(false);
    };
    let Some(item) = update_collection_item_feature_state(
        db,
        &collection.id,
        &item.id,
        "accepted",
        Some(approval_uri),
    )
    .await?
    else {
        return Ok(true);
    };
    enqueue_collection_item_add_activity(db, config, &owner, &collection.id, &item).await?;
    Ok(true)
}

pub(crate) async fn handle_inbox_collection_feature_reject(
    db: &crate::D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<bool> {
    let Some(feature_request_uri) = feature_response_object_uri(activity) else {
        return Ok(false);
    };
    let Some((collection, item)) =
        collection_item_by_feature_request_uri(db, feature_request_uri).await?
    else {
        return Ok(false);
    };
    if item.target_account_ref != remote_account_rest_id(&remote_actor.actor_uri) {
        return Ok(false);
    }
    let _ = update_collection_item_feature_state(db, &collection.id, &item.id, "rejected", None)
        .await?;
    Ok(true)
}

pub(crate) async fn handle_inbox_collection_feature_authorization_delete(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<bool> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(false);
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("FeatureAuthorization") {
        return Ok(false);
    }
    let Some(collection_uri) = object
        .get("interactingObject")
        .and_then(|value| activitypub_value_id(Some(value)))
    else {
        return Ok(false);
    };
    let Some(featured_actor_uri) = object
        .get("interactionTarget")
        .and_then(|value| activitypub_value_id(Some(value)))
    else {
        return Ok(false);
    };
    if featured_actor_uri != remote_actor.actor_uri {
        return Ok(false);
    }
    let Some(collection_id) = local_collection_id_from_uri(config, collection_uri) else {
        return Ok(false);
    };
    let Some(collection) = collection_row_by_id(db, &collection_id).await? else {
        return Ok(false);
    };
    let target_ref = remote_account_rest_id(featured_actor_uri);
    let row = db
        .prepare(
            "SELECT id
             FROM account_collection_items
             WHERE collection_id = ?1
               AND target_account_ref = ?2
             LIMIT 1",
        )
        .bind_refs(&[
            D1Type::Text(collection.id.as_str()),
            D1Type::Text(&target_ref),
        ])?
        .first::<serde_json::Value>(None)
        .await?;
    let Some(item_id) = row
        .as_ref()
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(true);
    };
    let _ =
        update_collection_item_feature_state(db, &collection.id, item_id, "revoked", None).await?;
    Ok(true)
}
