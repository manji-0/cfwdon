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

fn build_follow_response_object(
    config: &AppConfig,
    account: &LocalAccount,
    follow_activity_id: &str,
    remote_actor_uri: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": follow_activity_id,
        "type": "Follow",
        "actor": remote_actor_uri,
        "object": actor_url(config, &account.username),
    })
}

pub(crate) fn build_stored_accept_follow_activity(
    config: &AppConfig,
    account: &LocalAccount,
    follow_activity_id: &str,
    remote_actor_uri: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/accepts/{}", generate_entity_id(12)?),
        "type": "Accept",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": build_follow_response_object(config, account, follow_activity_id, remote_actor_uri),
    });
    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Accept activity: {error}")))
}

pub(crate) fn build_reject_follow_activity(
    config: &AppConfig,
    account: &LocalAccount,
    follow_activity_id: &str,
    remote_actor_uri: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/rejects/{}", generate_entity_id(12)?),
        "type": "Reject",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": build_follow_response_object(config, account, follow_activity_id, remote_actor_uri),
    });
    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Reject activity: {error}")))
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

fn quote_authorization_context() -> serde_json::Value {
    serde_json::json!([
        "https://www.w3.org/ns/activitystreams",
        {
            "QuoteAuthorization": "https://w3id.org/fep/044f#QuoteAuthorization",
            "interactingObject": "https://gotosocial.org/ns#interactingObject",
            "interactionTarget": "https://gotosocial.org/ns#interactionTarget",
        }
    ])
}

pub(crate) fn build_delete_quote_authorization_activity(
    config: &AppConfig,
    account: &LocalAccount,
    interacting_object_uri: &str,
    interaction_target_uri: &str,
    authorization_key: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let approval_id = format!(
        "{}/quote_authorizations/{}",
        interaction_target_uri.trim_end_matches('/'),
        authorization_key.trim()
    );
    let activity = serde_json::json!({
        "@context": quote_authorization_context(),
        "id": format!("{approval_id}#delete"),
        "type": "Delete",
        "actor": actor,
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "object": {
            "id": approval_id,
            "type": "QuoteAuthorization",
            "attributedTo": actor,
            "interactingObject": interacting_object_uri,
            "interactionTarget": interaction_target_uri,
        }
    });
    serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!(
            "failed to serialize Delete QuoteAuthorization activity: {error}"
        ))
    })
}

pub(crate) fn build_add_featured_activity(
    config: &AppConfig,
    account: &LocalAccount,
    status_uri: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let target = format!("{actor}/collections/featured");
    build_add_featured_activity_with_id(
        config,
        account,
        status_uri,
        &format!("{target}/add/{}", generate_entity_id(12)?),
    )
}

pub(crate) fn build_add_featured_activity_with_id(
    config: &AppConfig,
    account: &LocalAccount,
    status_uri: &str,
    activity_id: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let target = format!("{actor}/collections/featured");
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Add",
        "actor": actor,
        "object": status_uri,
        "target": target,
        "to": [format!("{}/followers", actor_url(config, &account.username))],
    });
    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Add activity: {error}")))
}

pub(crate) fn build_remove_featured_activity(
    config: &AppConfig,
    account: &LocalAccount,
    status_uri: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let target = format!("{actor}/collections/featured");
    build_remove_featured_activity_with_id(
        config,
        account,
        status_uri,
        &format!("{target}/remove/{}", generate_entity_id(12)?),
    )
}

pub(crate) fn build_remove_featured_activity_with_id(
    config: &AppConfig,
    account: &LocalAccount,
    status_uri: &str,
    activity_id: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let target = format!("{actor}/collections/featured");
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Remove",
        "actor": actor,
        "object": status_uri,
        "target": target,
        "to": [format!("{}/followers", actor_url(config, &account.username))],
    });
    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Remove activity: {error}")))
}
