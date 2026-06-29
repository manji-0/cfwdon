use super::{
    AppConfig, Error, LocalAccount, Result, StatusRow, actor_url, build_activitypub_actor_document,
    build_activitypub_note, generate_entity_id, now_iso_string,
};
use worker::D1Database;

fn status_activity_context(object: &serde_json::Value) -> serde_json::Value {
    if object.get("_misskey_quote").is_some() {
        serde_json::json!([
            "https://www.w3.org/ns/activitystreams",
            {
                "_misskey_quote": {
                    "@id": "https://misskey-hub.net/ns#_misskey_quote",
                    "@type": "@id"
                }
            }
        ])
    } else {
        serde_json::json!("https://www.w3.org/ns/activitystreams")
    }
}

pub(crate) fn build_update_person_activity_with_id(
    config: &AppConfig,
    account: &LocalAccount,
    activity_id: &str,
) -> Result<String> {
    let actor = actor_url(config, account.username());
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Update",
        "actor": actor,
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": [format!("{}/followers", actor_url(config, account.username()))],
        "object": build_activitypub_actor_document(config, account),
    });

    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Update activity: {error}")))
}

pub(crate) fn build_update_person_activity(
    config: &AppConfig,
    account: &LocalAccount,
) -> Result<String> {
    let actor = actor_url(config, account.username());
    build_update_person_activity_with_id(
        config,
        account,
        &format!("{actor}/updates/{}", generate_entity_id(12)?),
    )
}

pub(crate) async fn build_status_update_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<String> {
    let object = build_activitypub_note(db, config, account, status, false).await?;
    let object_id = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("activitypub status object id missing".to_owned()))?
        .to_owned();
    build_status_update_activity_with_id(
        config,
        account,
        object,
        &format!("{object_id}/updates/{}", generate_entity_id(12)?),
        &now_iso_string()?,
    )
}

pub(crate) fn build_status_update_activity_with_id(
    config: &AppConfig,
    account: &LocalAccount,
    object: serde_json::Value,
    activity_id: &str,
    published_at: &str,
) -> Result<String> {
    let actor = actor_url(config, account.username());
    let to = object
        .get("to")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let cc = object
        .get("cc")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let activity = serde_json::json!({
        "@context": status_activity_context(&object),
        "id": activity_id,
        "type": "Update",
        "actor": actor,
        "published": published_at,
        "to": to,
        "cc": cc,
        "object": object,
    });

    serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!(
            "failed to serialize status Update activity: {error}"
        ))
    })
}
