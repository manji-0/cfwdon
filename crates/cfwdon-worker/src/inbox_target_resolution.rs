use super::{
    AppConfig, D1Database, LocalAccount, activity_object_id, ensure_account_keys,
    extract_inbox_target_username, find_account_by_id, find_account_by_username,
    find_follow_by_activity_id, find_status_poll_vote_by_activity_uri,
    first_local_follower_for_remote_actor, is_activitypub_actor_type,
};
use worker::Result;

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
                .or(resolve_poll_vote_target_account(db, activity).await?)
                .or(resolve_remote_actor_update_target_account(db, activity).await?),
        },
    };

    match account {
        Some(account) => ensure_account_keys(db, account).await.map(Some),
        None => Ok(None),
    }
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
