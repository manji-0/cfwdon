use super::actor_url;
use super::generate_entity_id;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::{Error, Result};

pub(crate) fn build_poll_vote_activity(
    config: &AppConfig,
    account: &LocalAccount,
    remote_actor_uri: &str,
    question_uri: &str,
    option_title: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, account.username());
    let vote_id = format!("{actor}/votes/{}", generate_entity_id(12)?);
    let activity_id = format!("{vote_id}/activity");
    build_poll_vote_activity_with_ids(
        config,
        account,
        remote_actor_uri,
        question_uri,
        option_title,
        &vote_id,
        &activity_id,
    )
}

pub(crate) fn build_poll_vote_activity_with_ids(
    config: &AppConfig,
    account: &LocalAccount,
    remote_actor_uri: &str,
    question_uri: &str,
    option_title: &str,
    vote_id: &str,
    activity_id: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, account.username());
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Create",
        "to": [remote_actor_uri],
        "actor": actor,
        "object": {
            "id": vote_id,
            "type": "Note",
            "name": option_title,
            "attributedTo": actor_url(config, account.username()),
            "to": [remote_actor_uri],
            "inReplyTo": question_uri,
        }
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize poll vote activity: {error}"))
        })?,
    ))
}
