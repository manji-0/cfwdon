use super::{
    AppConfig, LocalAccount, actor_url, local_username_from_actor_uri,
    local_username_from_audience_uri, local_username_from_status_uri,
};

pub(crate) fn extract_inbox_target_username(
    config: &AppConfig,
    activity: &serde_json::Value,
) -> Option<String> {
    match activity.get("type").and_then(serde_json::Value::as_str) {
        Some("Follow") => activity_object_id(activity.get("object"))
            .and_then(|actor_uri| local_username_from_actor_uri(config, actor_uri)),
        Some("Accept") | Some("Reject") => activity
            .get("object")
            .and_then(|object| object.get("actor"))
            .and_then(serde_json::Value::as_str)
            .and_then(|actor_uri| local_username_from_actor_uri(config, actor_uri)),
        Some("Undo") => activity
            .get("object")
            .and_then(|object| object.get("object"))
            .and_then(|object| {
                activity_object_id(Some(object))
                    .and_then(|uri| {
                        local_username_from_actor_uri(config, uri)
                            .or_else(|| local_username_from_status_uri(config, uri))
                    })
                    .or_else(|| {
                        object
                            .get("inReplyTo")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|uri| local_username_from_status_uri(config, uri))
                    })
            }),
        Some("Like") | Some("Announce") => activity
            .get("object")
            .and_then(|object| activity_object_id(Some(object)))
            .and_then(|uri| local_username_from_status_uri(config, uri)),
        Some("Create") | Some("Update") => first_local_audience_username(config, activity),
        _ => None,
    }
}

pub(crate) fn activity_object_id(value: Option<&serde_json::Value>) -> Option<&str> {
    match value {
        Some(serde_json::Value::String(value)) => Some(value.as_str()),
        Some(serde_json::Value::Object(map)) => map.get("id").and_then(serde_json::Value::as_str),
        _ => None,
    }
}

pub(crate) fn first_local_audience_username(
    config: &AppConfig,
    activity: &serde_json::Value,
) -> Option<String> {
    for audience in activity_audience_uris(activity) {
        if let Some(username) = local_username_from_audience_uri(config, &audience) {
            return Some(username);
        }
    }

    None
}

pub(crate) fn activity_audience_uris(activity: &serde_json::Value) -> Vec<String> {
    let mut audiences = Vec::new();
    for key in ["to", "cc"] {
        collect_activitypub_uris(activity.get(key), &mut audiences);
        collect_activitypub_uris(
            activity.get("object").and_then(|object| object.get(key)),
            &mut audiences,
        );
    }
    audiences
}

pub(crate) fn collect_activitypub_uris(
    value: Option<&serde_json::Value>,
    audiences: &mut Vec<String>,
) {
    match value {
        Some(serde_json::Value::String(uri)) => audiences.push(uri.clone()),
        Some(serde_json::Value::Array(values)) => {
            for entry in values {
                collect_activitypub_uris(Some(entry), audiences);
            }
        }
        Some(serde_json::Value::Object(map)) => {
            if let Some(uri) = map.get("id").and_then(serde_json::Value::as_str) {
                audiences.push(uri.to_owned());
            }
        }
        _ => {}
    }
}

pub(crate) fn note_targets_account_or_followers(
    object: &serde_json::Value,
    account: &LocalAccount,
    config: &AppConfig,
) -> bool {
    let actor = actor_url(config, &account.username);
    let followers = format!("{actor}/followers");
    activity_audience_uris(&serde_json::json!({ "object": object }))
        .into_iter()
        .any(|audience| audience == actor || audience == followers)
}

pub(crate) fn contains_public_audience(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::String(value)) => {
            value == "https://www.w3.org/ns/activitystreams#Public"
        }
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .any(|value| contains_public_audience(Some(value))),
        Some(serde_json::Value::Object(map)) => map
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(|value| value == "https://www.w3.org/ns/activitystreams#Public")
            .unwrap_or(false),
        _ => false,
    }
}

pub(crate) fn follow_targets_local_actor(
    object: Option<&serde_json::Value>,
    local_actor_uri: &str,
) -> bool {
    match object {
        Some(serde_json::Value::String(value)) => value == local_actor_uri,
        Some(serde_json::Value::Object(map)) => map
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(|value| value == local_actor_uri)
            .unwrap_or(false),
        _ => false,
    }
}

pub(crate) fn is_follow_undo(
    object: Option<&serde_json::Value>,
    actor_uri: &str,
    canonical_actor_uri: &str,
) -> bool {
    match object {
        Some(serde_json::Value::String(_)) => true,
        Some(serde_json::Value::Object(map)) => {
            let is_follow = map
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(|value| value == "Follow")
                .unwrap_or(false);
            let object_actor = map
                .get("actor")
                .and_then(serde_json::Value::as_str)
                .map(|value| value == actor_uri || value == canonical_actor_uri)
                .unwrap_or(false);
            is_follow && object_actor
        }
        _ => false,
    }
}

pub(crate) fn is_activitypub_actor_type(actor_type: Option<&str>) -> bool {
    matches!(
        actor_type,
        Some("Person" | "Application" | "Group" | "Organization" | "Service")
    )
}

pub(crate) fn is_supported_remote_status_object_type(value: Option<&str>) -> bool {
    matches!(value, Some("Note" | "Question"))
}

pub(crate) fn extract_remote_note_object(
    document: &serde_json::Value,
) -> Option<&serde_json::Value> {
    if is_supported_remote_status_object_type(
        document.get("type").and_then(serde_json::Value::as_str),
    ) {
        return Some(document);
    }

    let object = document.get("object")?;
    if is_supported_remote_status_object_type(
        object.get("type").and_then(serde_json::Value::as_str),
    ) {
        Some(object)
    } else {
        None
    }
}

pub(crate) fn visibility_from_activitypub_object(object: &serde_json::Value) -> String {
    let to = object.get("to");
    let cc = object.get("cc");

    if contains_public_audience(to) {
        "public".to_owned()
    } else if contains_public_audience(cc) {
        "unlisted".to_owned()
    } else {
        "private".to_owned()
    }
}
