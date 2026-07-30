use super::{
    AppConfig, LocalAccount, actor_url, local_username_from_actor_uri,
    local_username_from_audience_uri, local_username_from_status_uri,
};

pub(crate) fn activitypub_type_strings(value: Option<&serde_json::Value>) -> Vec<&str> {
    match value {
        Some(serde_json::Value::String(value)) => vec![value.as_str()],
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn activitypub_has_type(object: &serde_json::Value, expected: &str) -> bool {
    activitypub_type_strings(object.get("type")).contains(&expected)
}

pub(crate) fn activitypub_primary_type(object: &serde_json::Value) -> Option<&str> {
    activitypub_type_strings(object.get("type"))
        .into_iter()
        .next()
}

pub(crate) fn extract_inbox_target_username(
    config: &AppConfig,
    activity: &serde_json::Value,
) -> Option<String> {
    let types = activitypub_type_strings(activity.get("type"));
    if types.contains(&"Follow") {
        activity_object_id(activity.get("object"))
            .and_then(|actor_uri| local_username_from_actor_uri(config, actor_uri))
    } else if types
        .iter()
        .any(|value| matches!(*value, "Accept" | "Reject"))
    {
        activity
            .get("object")
            .and_then(|object| object.get("actor"))
            .and_then(|actor| activity_object_id(Some(actor)))
            .and_then(|actor_uri| local_username_from_actor_uri(config, actor_uri))
    } else if types.contains(&"Undo") {
        activity
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
                            .and_then(|value| activity_object_id(Some(value)))
                            .and_then(|uri| local_username_from_status_uri(config, uri))
                    })
            })
    } else if types.contains(&"Like") {
        activity
            .get("object")
            .and_then(|object| activity_object_id(Some(object)))
            .and_then(|uri| local_username_from_status_uri(config, uri))
    } else if types.contains(&"Announce") {
        activity
            .get("object")
            .and_then(|object| {
                activity_object_id(Some(object))
                    .and_then(|uri| local_username_from_status_uri(config, uri))
                    .or_else(|| {
                        quote_target_uri_from_object(object)
                            .and_then(|uri| local_username_from_status_uri(config, &uri))
                    })
            })
            .or_else(|| first_local_audience_username(config, activity))
    } else if types
        .iter()
        .any(|value| matches!(*value, "Create" | "Update"))
    {
        first_local_audience_username(config, activity)
    } else {
        None
    }
}

pub(crate) fn activity_object_id(value: Option<&serde_json::Value>) -> Option<&str> {
    match value {
        Some(serde_json::Value::String(value)) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        }
        Some(serde_json::Value::Object(map)) => map
            .get("id")
            .or_else(|| map.get("@id"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .find_map(|entry| activity_object_id(Some(entry))),
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
            if let Some(uri) = map
                .get("id")
                .or_else(|| map.get("@id"))
                .and_then(serde_json::Value::as_str)
            {
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
    note_targets_account(object, account, config)
        || note_targets_followers(object, account, config)
        || note_targets_public(object)
}

pub(crate) fn note_targets_account(
    object: &serde_json::Value,
    account: &LocalAccount,
    config: &AppConfig,
) -> bool {
    let actor = actor_url(config, account.username());
    activity_audience_uris(&serde_json::json!({ "object": object }))
        .into_iter()
        .any(|audience| audience == actor)
}

/// True when the note addresses any followers collection (typically the author's).
///
/// Remote Creates use the remote actor's `/followers` URI in `to`/`cc`, not the
/// local viewer's. Matching only the local followers collection dropped Misskey
/// and Mastodon public/followers deliveries on the shared inbox path.
pub(crate) fn note_targets_followers(
    object: &serde_json::Value,
    _account: &LocalAccount,
    _config: &AppConfig,
) -> bool {
    activity_audience_uris(&serde_json::json!({ "object": object }))
        .into_iter()
        .any(|audience| cfwdon_domain::is_followers_collection_uri(&audience))
}

pub(crate) fn note_targets_public(object: &serde_json::Value) -> bool {
    activity_audience_uris(&serde_json::json!({ "object": object }))
        .into_iter()
        .any(|audience| cfwdon_domain::is_public_audience_uri(&audience))
}

pub(crate) fn contains_public_audience(value: Option<&serde_json::Value>) -> bool {
    fn is_public_audience_uri(value: &str) -> bool {
        matches!(
            value,
            "https://www.w3.org/ns/activitystreams#Public" | "as:Public"
        )
    }

    match value {
        Some(serde_json::Value::String(value)) => is_public_audience_uri(value),
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .any(|value| contains_public_audience(Some(value))),
        Some(serde_json::Value::Object(map)) => map
            .get("id")
            .or_else(|| map.get("@id"))
            .and_then(serde_json::Value::as_str)
            .map(is_public_audience_uri)
            .unwrap_or(false),
        _ => false,
    }
}

pub(crate) fn contains_followers_audience(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::String(value)) => cfwdon_domain::is_followers_collection_uri(value),
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .any(|value| contains_followers_audience(Some(value))),
        Some(serde_json::Value::Object(map)) => map
            .get("id")
            .or_else(|| map.get("@id"))
            .and_then(serde_json::Value::as_str)
            .map(cfwdon_domain::is_followers_collection_uri)
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
            .or_else(|| map.get("@id"))
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
        Some(serde_json::Value::String(_)) => false,
        Some(serde_json::Value::Object(map)) => {
            let is_follow = activitypub_type_strings(map.get("type")).contains(&"Follow");
            let object_actor = activity_object_id(map.get("actor"))
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

pub(crate) fn object_has_activitypub_actor_type(object: &serde_json::Value) -> bool {
    activitypub_type_strings(object.get("type"))
        .iter()
        .any(|value| is_activitypub_actor_type(Some(value)))
}

pub(crate) fn is_supported_remote_status_object_type(value: Option<&str>) -> bool {
    matches!(value, Some("Note" | "Question"))
}

pub(crate) fn object_has_supported_remote_status_type(object: &serde_json::Value) -> bool {
    activitypub_type_strings(object.get("type"))
        .iter()
        .any(|value| is_supported_remote_status_object_type(Some(value)))
}

pub(crate) fn object_attributed_to_remote_actor(
    object: &serde_json::Value,
    activity: &serde_json::Value,
    canonical_actor_uri: &str,
) -> bool {
    activity_object_id(object.get("attributedTo"))
        .or_else(|| activity_object_id(activity.get("actor")))
        .map(|value| value == canonical_actor_uri)
        .unwrap_or(false)
}

pub(crate) fn quote_target_uri_from_object(object: &serde_json::Value) -> Option<String> {
    cfwdon_domain::quote_target_uri_from_fields(
        object.get("quoteUri").and_then(serde_json::Value::as_str),
        object.get("quoteUrl").and_then(serde_json::Value::as_str),
        object
            .get("_misskey_quote")
            .and_then(serde_json::Value::as_str),
    )
}

pub(crate) fn extract_remote_note_object(
    document: &serde_json::Value,
) -> Option<&serde_json::Value> {
    if object_has_supported_remote_status_type(document) {
        return Some(document);
    }

    let object = document.get("object")?;
    if object_has_supported_remote_status_type(object) {
        Some(object)
    } else {
        None
    }
}

pub(crate) fn visibility_from_activitypub_object(object: &serde_json::Value) -> String {
    cfwdon_domain::visibility_from_activitypub_audiences(
        contains_public_audience(object.get("to")),
        contains_public_audience(object.get("cc")),
        contains_followers_audience(object.get("to"))
            || contains_followers_audience(object.get("cc")),
    )
    .as_str()
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activitypub_type_strings_accepts_string_and_array() {
        assert_eq!(
            activitypub_type_strings(Some(&serde_json::json!("Create"))),
            vec!["Create"]
        );
        assert_eq!(
            activitypub_type_strings(Some(&serde_json::json!(["Create", "Note"]))),
            vec!["Create", "Note"]
        );
        assert!(activitypub_type_strings(Some(&serde_json::json!(1))).is_empty());
    }

    #[test]
    fn activitypub_has_type_matches_array_members() {
        let activity = serde_json::json!({"type": ["Create"]});
        assert!(activitypub_has_type(&activity, "Create"));
        assert!(!activitypub_has_type(&activity, "Delete"));
    }

    #[test]
    fn activity_object_id_accepts_embedded_object_and_array() {
        assert_eq!(
            activity_object_id(Some(&serde_json::json!({
                "id": "https://remote.example/users/bob",
                "type": "Person"
            }))),
            Some("https://remote.example/users/bob")
        );
        assert_eq!(
            activity_object_id(Some(&serde_json::json!({
                "@id": "https://remote.example/users/carol"
            }))),
            Some("https://remote.example/users/carol")
        );
        assert_eq!(
            activity_object_id(Some(&serde_json::json!([
                {"id": "https://remote.example/users/dave"},
                "https://remote.example/users/eve"
            ]))),
            Some("https://remote.example/users/dave")
        );
        assert_eq!(
            activity_object_id(Some(&serde_json::json!({"type": "Person"}))),
            None
        );
    }
}
