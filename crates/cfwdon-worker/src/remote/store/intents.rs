use crate::remote::adapters::{
    activity_pub_reblog_input_from_activity, activity_pub_status_input_from_object,
};
use crate::{RemoteActorProfile, now_iso_string};
use cfwdon_domain::{
    RemoteQuoteResolution, StatusId, StoredRemoteReblogIntent, StoredRemoteStatusIntent,
};
use worker::{Error, Result};

pub(super) fn serialize_remote_store_json(
    value: &serde_json::Value,
    label: &str,
) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|error| Error::RustError(format!("failed to serialize {label}: {error}")))
}

pub(super) fn serialize_remote_status_object_json(object: &serde_json::Value) -> Result<String> {
    serialize_remote_store_json(object, "remote status object")
}

pub(super) fn serialize_remote_reblog_activity_json(
    activity: &serde_json::Value,
) -> Result<String> {
    serialize_remote_store_json(activity, "remote announce activity")
}

pub(super) fn serialize_remote_status_snapshot_json(
    snapshot: &serde_json::Value,
) -> Result<String> {
    serialize_remote_store_json(snapshot, "remote status snapshot")
}

pub(super) fn build_remote_status_store_intent(
    actor: &RemoteActorProfile,
    object: &serde_json::Value,
    status_id: StatusId,
    quote_resolution: RemoteQuoteResolution,
    revision_at: String,
) -> Result<StoredRemoteStatusIntent> {
    let input = activity_pub_status_input_from_object(object);
    let incoming = input
        .into_incoming()
        .map_err(|error| Error::RustError(error.to_string()))?;
    Ok(incoming
        .into_store_intent(
            status_id,
            actor.actor_uri.clone(),
            quote_resolution,
            serialize_remote_status_object_json(object)?,
            revision_at,
        )
        .state)
}

pub(super) fn build_remote_reblog_store_intent(
    actor: &RemoteActorProfile,
    activity: &serde_json::Value,
    status_id: StatusId,
    quote_resolution: RemoteQuoteResolution,
) -> Result<StoredRemoteReblogIntent> {
    let input = activity_pub_reblog_input_from_activity(activity);
    let incoming = input
        .into_incoming()
        .map_err(|error| Error::RustError(error.to_string()))?;
    let mut intent = incoming
        .into_store_intent(
            status_id,
            actor.actor_uri.clone(),
            quote_resolution,
            serialize_remote_reblog_activity_json(activity)?,
        )
        .state;
    if intent.published_at.is_empty() {
        intent.published_at = now_iso_string()?;
    }
    Ok(intent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn remote_actor_profile_fixture() -> RemoteActorProfile {
        RemoteActorProfile {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            inbox_uri: "https://remote.example/users/alice/inbox".to_owned(),
            shared_inbox_uri: Some("https://remote.example/inbox".to_owned()),
            public_key_id: "https://remote.example/users/alice#main-key".to_owned(),
            public_key_pem: "pem".to_owned(),
            display_name: "Alice".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@alice".to_owned()),
            avatar_url: None,
            header_url: None,
        }
    }

    #[test]
    fn incoming_remote_status_requires_object_id() {
        let error = activity_pub_status_input_from_object(&json!({}))
            .into_incoming()
            .unwrap_err();

        assert!(error.to_string().contains("missing id"));
    }

    #[test]
    fn serialize_remote_status_object_json_preserves_status_payload() {
        let object = json!({
            "type": "Note",
            "content": "<p>Hello</p>",
            "sensitive": false
        });

        let json = serialize_remote_status_object_json(&object).unwrap();

        assert_eq!(
            json,
            "{\"content\":\"<p>Hello</p>\",\"sensitive\":false,\"type\":\"Note\"}"
        );
    }

    #[test]
    fn serialize_remote_reblog_activity_json_preserves_announce_payload() {
        let activity = json!({
            "type": "Announce",
            "object": "https://remote.example/users/bob/statuses/9"
        });

        let json = serialize_remote_reblog_activity_json(&activity).unwrap();

        assert_eq!(
            json,
            "{\"object\":\"https://remote.example/users/bob/statuses/9\",\"type\":\"Announce\"}"
        );
    }

    #[test]
    fn serialize_remote_status_snapshot_json_preserves_history_payload() {
        let snapshot = json!({
            "created_at": "2026-05-10T01:02:03Z",
            "content": "<p>Before</p>"
        });

        let json = serialize_remote_status_snapshot_json(&snapshot).unwrap();

        assert_eq!(
            json,
            "{\"content\":\"<p>Before</p>\",\"created_at\":\"2026-05-10T01:02:03Z\"}"
        );
    }

    #[test]
    fn build_remote_status_store_intent_extracts_storage_fields() {
        use cfwdon_domain::{QuoteState, RemoteQuoteResolution, StatusId, Visibility};

        let actor = remote_actor_profile_fixture();
        let object = json!({
            "id": "https://remote.example/users/alice/statuses/1",
            "url": "https://remote.example/@alice/1",
            "inReplyTo": "https://remote.example/users/bob/statuses/9",
            "content": "<p>Hello</p>",
            "summary": "spoiler",
            "sensitive": true,
            "published": "2026-05-10T01:02:03Z",
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "contentMap": {
                "ja": "<p>Hello</p>"
            }
        });

        let intent = build_remote_status_store_intent(
            &actor,
            &object,
            StatusId::new("remote-status-id").expect("status id"),
            RemoteQuoteResolution::without_quote(),
            "revision-time".to_owned(),
        )
        .expect("store intent");

        assert_eq!(intent.actor_uri, actor.actor_uri);
        assert_eq!(
            intent.object_uri,
            "https://remote.example/users/alice/statuses/1"
        );
        assert_eq!(
            intent.url.as_deref(),
            Some("https://remote.example/@alice/1")
        );
        assert_eq!(
            intent.in_reply_to_uri.as_deref(),
            Some("https://remote.example/users/bob/statuses/9")
        );
        assert_eq!(intent.content_html, "<p>Hello</p>");
        assert_eq!(intent.spoiler_text, "spoiler");
        assert_eq!(intent.visibility, Visibility::Public);
        assert!(intent.sensitive);
        assert_eq!(intent.language.as_deref(), Some("ja"));
        assert_eq!(intent.quote_state, QuoteState::Accepted);
        assert_eq!(intent.published_at, "2026-05-10T01:02:03Z");
        assert_eq!(intent.revision_at, "revision-time");
        assert_eq!(intent.status_id.as_str(), "remote-status-id");
    }

    #[test]
    fn build_remote_reblog_store_intent_extracts_storage_fields() {
        use cfwdon_domain::{QuoteState, RemoteQuoteResolution, StatusId, Visibility};

        let actor = remote_actor_profile_fixture();
        let activity = json!({
            "id": "https://remote.example/users/alice/activities/announce/1",
            "type": "Announce",
            "object": {
                "id": "https://remote.example/users/bob/statuses/9"
            },
            "quoteUri": "https://local.example/users/alice/statuses/2",
            "published": "2026-05-11T01:02:03Z",
            "to": ["https://www.w3.org/ns/activitystreams#Public"]
        });

        let intent = build_remote_reblog_store_intent(
            &actor,
            &activity,
            StatusId::new("remote-reblog-id").expect("status id"),
            RemoteQuoteResolution::accepted_quote(
                "https://local.example/users/alice/statuses/2".to_owned(),
            ),
        )
        .expect("store intent");

        assert_eq!(intent.status_id.as_str(), "remote-reblog-id");
        assert_eq!(intent.actor_uri, actor.actor_uri);
        assert_eq!(
            intent.object_uri,
            "https://remote.example/users/alice/activities/announce/1"
        );
        assert_eq!(
            intent.boost_of_uri,
            "https://remote.example/users/bob/statuses/9"
        );
        assert_eq!(
            intent.quote_of_uri.as_deref(),
            Some("https://local.example/users/alice/statuses/2")
        );
        assert_eq!(intent.visibility, Visibility::Public);
        assert_eq!(intent.quote_state, QuoteState::Accepted);
        assert_eq!(intent.published_at, "2026-05-11T01:02:03Z");
        assert!(intent.raw_object_json.contains("\"Announce\""));
    }

    #[test]
    fn build_remote_reblog_store_intent_fills_missing_published_at() {
        use cfwdon_domain::{RemoteQuoteResolution, StatusId};

        let actor = remote_actor_profile_fixture();
        let activity = json!({
            "id": "https://remote.example/users/alice/activities/announce/1",
            "type": "Announce",
            "object": "https://remote.example/users/bob/statuses/9"
        });

        let intent = build_remote_reblog_store_intent(
            &actor,
            &activity,
            StatusId::new("remote-reblog-id").expect("status id"),
            RemoteQuoteResolution::without_quote(),
        )
        .expect("store intent");

        assert!(!intent.published_at.is_empty());
    }

    #[test]
    fn activity_pub_status_input_prefers_content_html() {
        let input = activity_pub_status_input_from_object(&json!({
            "content": "<p>Remote content</p>",
            "name": "Fallback name",
        }));

        assert_eq!(input.content_html, "<p>Remote content</p>");
    }

    #[test]
    fn incoming_remote_status_maps_published_at_and_language() {
        let incoming = activity_pub_status_input_from_object(&json!({
            "id": "https://remote.example/statuses/1",
            "published": "2026-05-10T01:02:03Z",
            "updated": "2026-05-10T04:05:06Z",
            "contentMap": {
                "ja": "こんにちは",
            },
        }))
        .into_incoming()
        .expect("incoming status");

        assert_eq!(incoming.published_at(), "2026-05-10T01:02:03Z");
        assert_eq!(incoming.language().as_deref(), Some("ja"));
    }

    #[test]
    fn incoming_remote_status_falls_back_to_updated_timestamp() {
        let incoming = activity_pub_status_input_from_object(&json!({
            "id": "https://remote.example/statuses/1",
            "updated": "2026-05-10T04:05:06Z",
        }))
        .into_incoming()
        .expect("incoming status");

        assert_eq!(incoming.published_at(), "2026-05-10T04:05:06Z");
    }
}
