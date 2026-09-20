use super::{
    CollectionItemRow, CollectionRow, RemoteCollectionItemRow, RemoteCollectionRow,
    collection_item_uri, collection_uri, list_collection_items,
    update_collection_item_feature_request_uri,
};
use crate::{
    AccountReference, Result, actor_url, enqueue_targeted_outbox_activity, instance_base_url,
    list_follower_delivery_targets, queue_remote_actor_activity, resolve_account_reference,
    timestamp_to_mastodon_iso8601,
};

async fn account_actor_uri_for_reference(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ref: &str,
) -> Result<Option<String>> {
    match resolve_account_reference(db, account_ref).await? {
        Some(AccountReference::Local(account)) => Ok(Some(actor_url(config, account.username()))),
        Some(AccountReference::Remote(actor)) => Ok(Some(actor.actor_uri)),
        None => Ok(None),
    }
}

async fn collection_item_activitypub_object(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
) -> Result<Option<serde_json::Value>> {
    let Some(featured_object) =
        account_actor_uri_for_reference(db, config, &item.target_account_ref).await?
    else {
        return Ok(None);
    };
    let item_uri = collection_item_uri(config, owner, collection_id, &item.id);
    let feature_authorization = item
        .feature_authorization
        .clone()
        .unwrap_or_else(|| format!("{item_uri}/feature_authorization"));
    Ok(Some(serde_json::json!({
        "id": item_uri,
        "type": "FeaturedItem",
        "featuredObject": featured_object,
        "featuredObjectType": "Person",
        "featureAuthorization": feature_authorization,
        "published": timestamp_to_mastodon_iso8601(&item.created_at),
    })))
}

async fn collection_activitypub_object(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
) -> Result<serde_json::Value> {
    let item_rows = list_collection_items(db, &row.id, false).await?;
    let mut ordered_items = Vec::new();
    for item in &item_rows {
        if let Some(object) =
            collection_item_activitypub_object(db, config, owner, &row.id, item).await?
        {
            ordered_items.push(object);
        }
    }

    let uri = collection_uri(config, owner, &row.id);
    let mut object = serde_json::json!({
        "id": uri,
        "type": "FeaturedCollection",
        "totalItems": ordered_items.len(),
        "name": row.name,
        "attributedTo": actor_url(config, owner.username()),
        "url": uri,
        "sensitive": row.sensitive,
        "discoverable": row.discoverable != 0,
        "published": timestamp_to_mastodon_iso8601(&row.created_at),
        "updated": timestamp_to_mastodon_iso8601(&row.updated_at),
        "orderedItems": ordered_items,
    });
    if let Some(language) = row.language.as_deref().filter(|value| !value.is_empty()) {
        object["summaryMap"] = serde_json::json!({ language: row.description });
    } else {
        object["summary"] = serde_json::json!(row.description);
    }
    if let Some(tag_name) = row.tag_name.as_deref().filter(|value| !value.is_empty()) {
        object["topic"] = serde_json::json!({
            "type": "Hashtag",
            "name": format!("#{tag_name}"),
            "href": format!("{}/tags/{tag_name}", instance_base_url(config)),
        });
    }
    Ok(object)
}

async fn enqueue_collection_followers_activity(
    db: &crate::D1Database,
    _config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    _collection_id: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let follower_inboxes = list_follower_delivery_targets(db, owner.id()).await?;
    if follower_inboxes.is_empty() {
        return Ok(());
    }
    let payload_json = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize collection activity: {error}"))
    })?;
    enqueue_targeted_outbox_activity(db, owner.id(), None, &payload_json, &follower_inboxes).await
}

pub(in crate::collections_alpha) async fn enqueue_collection_add_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
) -> Result<()> {
    let actor = actor_url(config, owner.username());
    let collection_uri = collection_uri(config, owner, &row.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{collection_uri}#add"),
        "type": "Add",
        "actor": actor,
        "object": collection_activitypub_object(db, config, owner, row).await?,
        "target": format!("{actor}/collections/featured"),
        "to": [format!("{actor}/followers")],
    });
    enqueue_collection_followers_activity(db, config, owner, &row.id, payload).await
}

pub(in crate::collections_alpha) async fn enqueue_collection_update_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
) -> Result<()> {
    let collection_uri = collection_uri(config, owner, &row.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{collection_uri}#updates/{}", row.updated_at),
        "type": "Update",
        "actor": actor_url(config, owner.username()),
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "object": collection_activitypub_object(db, config, owner, row).await?,
    });
    enqueue_collection_followers_activity(db, config, owner, &row.id, payload).await
}

pub(in crate::collections_alpha) async fn enqueue_collection_remove_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
) -> Result<()> {
    let actor = actor_url(config, owner.username());
    let collection_uri = collection_uri(config, owner, &row.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{collection_uri}#remove"),
        "type": "Remove",
        "actor": actor,
        "object": collection_uri,
        "target": format!("{actor}/collections/featured"),
        "to": [format!("{actor}/followers")],
    });
    enqueue_collection_followers_activity(db, config, owner, &row.id, payload).await
}

pub(in crate::collections_alpha) async fn enqueue_collection_item_add_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
) -> Result<()> {
    let Some(object) =
        collection_item_activitypub_object(db, config, owner, collection_id, item).await?
    else {
        return Ok(());
    };
    let item_uri = collection_item_uri(config, owner, collection_id, &item.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{item_uri}#add"),
        "type": "Add",
        "actor": actor_url(config, owner.username()),
        "object": object,
        "target": collection_uri(config, owner, collection_id),
    });
    enqueue_collection_followers_activity(db, config, owner, collection_id, payload).await
}

pub(in crate::collections_alpha) fn build_collection_feature_request_activity(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
    remote_actor_uri: &str,
) -> serde_json::Value {
    serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": collection_feature_request_uri(config, owner, collection_id, &item.id),
        "type": "FeatureRequest",
        "object": remote_actor_uri,
        "instrument": collection_uri(config, owner, collection_id),
    })
}

fn collection_feature_request_uri(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item_id: &str,
) -> String {
    format!(
        "{}#feature_request",
        collection_item_uri(config, owner, collection_id, item_id)
    )
}

pub(in crate::collections_alpha) async fn enqueue_collection_feature_request_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
    remote_actor_uri: &str,
) -> Result<()> {
    let activity_uri = collection_feature_request_uri(config, owner, collection_id, &item.id);
    update_collection_item_feature_request_uri(db, collection_id, &item.id, &activity_uri).await?;
    let payload = build_collection_feature_request_activity(
        config,
        owner,
        collection_id,
        item,
        remote_actor_uri,
    );
    let payload_json = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize collection feature request: {error}"
        ))
    })?;
    let _ = queue_remote_actor_activity(db, owner.id(), remote_actor_uri, &payload_json).await?;
    Ok(())
}

pub(in crate::collections_alpha) async fn enqueue_collection_item_remove_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item: &CollectionItemRow,
) -> Result<()> {
    let item_uri = collection_item_uri(config, owner, collection_id, &item.id);
    let payload = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{item_uri}#remove"),
        "type": "Remove",
        "actor": actor_url(config, owner.username()),
        "object": item_uri,
        "target": collection_uri(config, owner, collection_id),
    });
    enqueue_collection_followers_activity(db, config, owner, collection_id, payload).await
}

pub(in crate::collections_alpha) fn build_delete_feature_authorization_activity(
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    collection: &RemoteCollectionRow,
    item: &RemoteCollectionItemRow,
) -> serde_json::Value {
    let feature_authorization = item
        .feature_authorization
        .clone()
        .unwrap_or_else(|| format!("{}/feature_authorization", item.id));
    let actor = actor_url(config, requester.username());
    serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{feature_authorization}#delete"),
        "type": "Delete",
        "actor": actor,
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "object": {
            "id": feature_authorization,
            "type": "FeatureAuthorization",
            "interactingObject": collection.uri,
            "interactionTarget": actor,
        },
    })
}

pub(in crate::collections_alpha) async fn enqueue_delete_feature_authorization_activity(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    collection: &RemoteCollectionRow,
    item: &RemoteCollectionItemRow,
) -> Result<()> {
    let payload = build_delete_feature_authorization_activity(config, requester, collection, item);
    let payload_json = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize delete feature authorization activity: {error}"
        ))
    })?;
    let _ = queue_remote_actor_activity(db, requester.id(), &collection.actor_uri, &payload_json)
        .await?;
    let follower_inboxes = list_follower_delivery_targets(db, requester.id()).await?;
    if !follower_inboxes.is_empty() {
        enqueue_targeted_outbox_activity(
            db,
            requester.id(),
            None,
            &payload_json,
            &follower_inboxes,
        )
        .await?;
    }
    Ok(())
}
