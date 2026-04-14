use super::{LocalAccount, activitypub_audiences, actor_url, generate_entity_id, now_iso_string};
use cfwdon_core::AppConfig;
use worker::{Error, Result};

pub(crate) fn build_accept_activity(
    config: &AppConfig,
    account: &LocalAccount,
    follow_activity: &serde_json::Value,
    remote_actor_uri: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/accepts/{}", generate_entity_id(12)?),
        "type": "Accept",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": follow_activity,
    });
    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Accept activity: {error}")))
}

pub(crate) fn build_follow_activity(
    config: &AppConfig,
    account: &LocalAccount,
    remote_actor_uri: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let follow_activity_id = format!("{actor}/follows/{}", generate_entity_id(12)?);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": follow_activity_id,
        "type": "Follow",
        "actor": actor,
        "object": remote_actor_uri,
        "to": [remote_actor_uri],
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize Follow activity: {error}"))
        })?,
    ))
}

pub(crate) fn build_undo_follow_activity(
    config: &AppConfig,
    account: &LocalAccount,
    follow_activity_id: &str,
    remote_actor_uri: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/undo/{}", generate_entity_id(12)?),
        "type": "Undo",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": {
            "id": follow_activity_id,
            "type": "Follow",
            "actor": actor,
            "object": remote_actor_uri,
        }
    });
    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Undo activity: {error}")))
}

pub(crate) fn build_like_activity(
    config: &AppConfig,
    account: &LocalAccount,
    remote_actor_uri: &str,
    object_uri: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let activity_id = format!("{actor}/likes/{}", generate_entity_id(12)?);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Like",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": object_uri,
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize Like activity: {error}"))
        })?,
    ))
}

pub(crate) fn build_undo_like_activity(
    config: &AppConfig,
    account: &LocalAccount,
    like_activity_id: &str,
    remote_actor_uri: &str,
    object_uri: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/undo/{}", generate_entity_id(12)?),
        "type": "Undo",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": {
            "id": like_activity_id,
            "type": "Like",
            "actor": actor,
            "object": object_uri,
        }
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize Undo Like activity: {error}"))
        })?,
    ))
}

pub(crate) fn build_announce_activity(
    config: &AppConfig,
    account: &LocalAccount,
    object_uri: &str,
    visibility: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let audiences = activitypub_audiences(config, &account.username, visibility);
    let activity_id = format!("{actor}/announces/{}", generate_entity_id(12)?);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Announce",
        "actor": actor,
        "published": now_iso_string()?,
        "to": audiences.0,
        "cc": audiences.1,
        "object": object_uri,
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize Announce activity: {error}"))
        })?,
    ))
}

pub(crate) fn build_undo_announce_activity(
    config: &AppConfig,
    account: &LocalAccount,
    announce_activity_id: &str,
    remote_actor_uri: &str,
    object_uri: &str,
    visibility: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let audiences = activitypub_audiences(config, &account.username, visibility);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/undo/{}", generate_entity_id(12)?),
        "type": "Undo",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": {
            "id": announce_activity_id,
            "type": "Announce",
            "actor": actor,
            "to": audiences.0,
            "cc": audiences.1,
            "object": object_uri,
        }
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!(
                "failed to serialize Undo Announce activity: {error}"
            ))
        })?,
    ))
}
