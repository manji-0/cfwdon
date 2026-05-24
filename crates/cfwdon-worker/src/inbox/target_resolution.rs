use super::{
    AppConfig, D1Database, LocalAccount, activity_object_id, ensure_account_keys,
    extract_inbox_target_username, find_account_by_id, find_account_by_username,
    find_follow_by_activity_id, find_local_status_by_object_uri,
    find_status_poll_vote_by_activity_uri, first_local_follower_for_remote_actor,
    is_activitypub_actor_type, is_supported_remote_status_object_type,
    quote_target_uri_from_object,
};
use worker::Result;
use worker::d1::D1Type;

pub(crate) async fn resolve_inbox_target_account(
    db: &D1Database,
    config: &AppConfig,
    username: Option<&str>,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    let account = match username {
        Some(username) => find_account_by_username(db, username).await?,
        None => match extract_inbox_target_username(config, activity) {
            Some(target_username) => find_account_by_username(db, &target_username).await?,
            None => resolve_follow_response_target_account(db, activity)
                .await?
                .or(resolve_feature_response_target_account(db, activity).await?)
                .or(resolve_poll_vote_target_account(db, activity).await?)
                .or(resolve_remote_status_activity_target_account(db, config, activity).await?)
                .or(resolve_remote_actor_update_target_account(db, activity).await?)
                .or(resolve_remote_actor_announce_target_account(db, activity).await?)
                .or(
                    resolve_feature_authorization_delete_target_account(db, config, activity)
                        .await?,
                )
                .or(resolve_remote_collection_activity_target_account(db, activity).await?),
        },
    };

    match account {
        Some(account) => ensure_account_keys(db, config, account).await.map(Some),
        None => Ok(None),
    }
}

async fn find_first_account_id_by_query(
    db: &D1Database,
    sql: &str,
    value: &str,
) -> Result<Option<String>> {
    let row = db
        .prepare(sql)
        .bind_refs(&[D1Type::Text(value)])?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.and_then(|value| {
        value
            .get("account_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    }))
}

async fn resolve_local_status_owner_account(
    db: &D1Database,
    config: &AppConfig,
    status_uri: &str,
) -> Result<Option<LocalAccount>> {
    let Some(status) = find_local_status_by_object_uri(db, config, status_uri).await? else {
        return Ok(None);
    };
    find_account_by_id(db, &status.account_id).await
}

async fn resolve_local_interaction_target_account(
    db: &D1Database,
    remote_status_uri: &str,
) -> Result<Option<LocalAccount>> {
    if let Some(account_id) = find_first_account_id_by_query(
        db,
        "SELECT account_id
         FROM statuses
         WHERE quote_of_uri = ?1
           AND COALESCE(quote_state, 'accepted') != 'revoked'
         LIMIT 1",
        remote_status_uri,
    )
    .await?
    {
        return find_account_by_id(db, &account_id).await;
    }

    if let Some(account_id) = find_first_account_id_by_query(
        db,
        "SELECT account_id FROM reblogs WHERE target_uri = ?1 LIMIT 1",
        remote_status_uri,
    )
    .await?
    {
        return find_account_by_id(db, &account_id).await;
    }

    Ok(None)
}

pub(crate) async fn resolve_remote_status_activity_target_account(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    if !matches!(
        activity.get("type").and_then(serde_json::Value::as_str),
        Some("Create" | "Update")
    ) {
        return Ok(None);
    }
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(None);
    };
    if !is_supported_remote_status_object_type(
        object.get("type").and_then(serde_json::Value::as_str),
    ) {
        return Ok(None);
    }

    if let Some(status_uri) = activity_object_id(Some(object)) {
        if let Some(account) = resolve_local_status_owner_account(db, config, status_uri).await? {
            return Ok(Some(account));
        }
        if let Some(account) = resolve_local_interaction_target_account(db, status_uri).await? {
            return Ok(Some(account));
        }
    }

    if let Some(in_reply_to_uri) = object.get("inReplyTo").and_then(serde_json::Value::as_str)
        && let Some(account) =
            resolve_local_status_owner_account(db, config, in_reply_to_uri).await?
    {
        return Ok(Some(account));
    }

    if let Some(quote_uri) = quote_target_uri_from_object(object)
        && let Some(account) = resolve_local_status_owner_account(db, config, &quote_uri).await?
    {
        return Ok(Some(account));
    }

    let Some(actor_uri) = activity
        .get("actor")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            object
                .get("attributedTo")
                .and_then(serde_json::Value::as_str)
        })
    else {
        return Ok(None);
    };

    first_local_follower_for_remote_actor(db, actor_uri).await
}

pub(crate) async fn resolve_remote_actor_update_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    if activity.get("type").and_then(serde_json::Value::as_str) != Some("Update") {
        return Ok(None);
    }
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(None);
    };
    if !is_activitypub_actor_type(object.get("type").and_then(serde_json::Value::as_str)) {
        return Ok(None);
    }
    let Some(actor_uri) = activity_object_id(Some(object))
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
    else {
        return Ok(None);
    };

    first_local_follower_for_remote_actor(db, actor_uri).await
}

pub(crate) async fn resolve_remote_actor_announce_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    if activity.get("type").and_then(serde_json::Value::as_str) != Some("Announce") {
        return Ok(None);
    }
    let Some(actor_uri) = activity
        .get("actor")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };

    first_local_follower_for_remote_actor(db, actor_uri).await
}

pub(crate) async fn resolve_remote_collection_activity_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    let is_collection_activity = match activity.get("type").and_then(serde_json::Value::as_str) {
        Some("Add" | "Remove") => activity.get("target").is_some(),
        Some("Update") => {
            activity
                .get("object")
                .and_then(|object| object.get("type"))
                .and_then(serde_json::Value::as_str)
                == Some("FeaturedCollection")
        }
        _ => false,
    };
    if !is_collection_activity {
        return Ok(None);
    }
    let Some(actor_uri) = activity
        .get("actor")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };

    first_local_follower_for_remote_actor(db, actor_uri).await
}

pub(crate) async fn resolve_follow_response_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    let Some(follow_activity_id) = activity
        .get("object")
        .and_then(|object| activity_object_id(Some(object)))
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    let Some(follow) = find_follow_by_activity_id(db, &follow_activity_id).await? else {
        return Ok(None);
    };

    find_account_by_id(db, &follow.follower_account_id).await
}

pub(crate) async fn resolve_feature_response_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    if !matches!(
        activity.get("type").and_then(serde_json::Value::as_str),
        Some("Accept" | "Reject")
    ) {
        return Ok(None);
    }
    let Some(feature_request_uri) = activity
        .get("object")
        .and_then(|object| activity_object_id(Some(object)))
    else {
        return Ok(None);
    };
    let Some(account_id) = find_first_account_id_by_query(
        db,
        "SELECT c.account_id
         FROM account_collection_items i
         JOIN account_collections c
           ON c.id = i.collection_id
         WHERE i.activity_uri = ?1
         LIMIT 1",
        feature_request_uri,
    )
    .await?
    else {
        return Ok(None);
    };

    find_account_by_id(db, &account_id).await
}

pub(crate) async fn resolve_feature_authorization_delete_target_account(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    if activity.get("type").and_then(serde_json::Value::as_str) != Some("Delete") {
        return Ok(None);
    }
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(None);
    };
    if object.get("type").and_then(serde_json::Value::as_str) != Some("FeatureAuthorization") {
        return Ok(None);
    }
    let Some(collection_uri) = object
        .get("interactingObject")
        .and_then(|value| activity_object_id(Some(value)))
    else {
        return Ok(None);
    };
    let Some(collection_id) = crate::local_collection_id_from_uri(config, collection_uri) else {
        return Ok(None);
    };
    let Some(account_id) = find_first_account_id_by_query(
        db,
        "SELECT account_id
         FROM account_collections
         WHERE id = ?1
         LIMIT 1",
        &collection_id,
    )
    .await?
    else {
        return Ok(None);
    };

    find_account_by_id(db, &account_id).await
}

pub(crate) async fn resolve_poll_vote_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    if activity.get("type").and_then(serde_json::Value::as_str) != Some("Undo") {
        return Ok(None);
    }
    let Some(activity_uri) = activity
        .get("object")
        .and_then(|object| activity_object_id(Some(object)))
    else {
        return Ok(None);
    };
    let Some(vote) = find_status_poll_vote_by_activity_uri(db, activity_uri).await? else {
        return Ok(None);
    };

    find_account_by_id(db, &vote.status_account_id).await
}
