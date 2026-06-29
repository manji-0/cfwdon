use cfwdon_domain::{ActivityPubReblogInput, ActivityPubStatusInput};
use serde_json::Value;

use crate::render_status_html;

pub(crate) fn activity_pub_status_input_from_object(object: &Value) -> ActivityPubStatusInput {
    ActivityPubStatusInput {
        object_id: object.get("id").and_then(Value::as_str).map(str::to_owned),
        url: object.get("url").and_then(Value::as_str).map(str::to_owned),
        in_reply_to: object
            .get("inReplyTo")
            .and_then(Value::as_str)
            .map(str::to_owned),
        quote_uri: object
            .get("quoteUri")
            .and_then(Value::as_str)
            .map(str::to_owned),
        quote_url: object
            .get("quoteUrl")
            .and_then(Value::as_str)
            .map(str::to_owned),
        misskey_quote: object
            .get("_misskey_quote")
            .and_then(Value::as_str)
            .map(str::to_owned),
        content_html: remote_status_content_html(object),
        spoiler_text: object
            .get("summary")
            .and_then(Value::as_str)
            .map(str::to_owned),
        to_audiences: collect_activitypub_audiences(object.get("to")),
        cc_audiences: collect_activitypub_audiences(object.get("cc")),
        sensitive: object.get("sensitive").and_then(Value::as_bool),
        language: remote_status_language(object),
        published_at: object
            .get("published")
            .and_then(Value::as_str)
            .map(str::to_owned),
        updated_at: object
            .get("updated")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

pub(crate) fn activity_pub_reblog_input_from_activity(activity: &Value) -> ActivityPubReblogInput {
    ActivityPubReblogInput {
        activity_id: activity
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        boost_of_uri: activity
            .get("object")
            .and_then(|value| crate::activity_object_id(Some(value)))
            .map(str::to_owned),
        quote_uri: activity
            .get("quoteUri")
            .and_then(Value::as_str)
            .map(str::to_owned),
        quote_url: activity
            .get("quoteUrl")
            .and_then(Value::as_str)
            .map(str::to_owned),
        misskey_quote: activity
            .get("_misskey_quote")
            .and_then(Value::as_str)
            .map(str::to_owned),
        to_audiences: collect_activitypub_audiences(activity.get("to")),
        cc_audiences: collect_activitypub_audiences(activity.get("cc")),
        published_at: activity
            .get("published")
            .and_then(Value::as_str)
            .map(str::to_owned),
        updated_at: activity
            .get("updated")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn remote_status_content_html(object: &Value) -> String {
    object
        .get("content")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            object
                .get("name")
                .and_then(Value::as_str)
                .map(render_status_html)
        })
        .unwrap_or_default()
}

fn remote_status_language(object: &Value) -> Option<String> {
    object
        .get("contentMap")
        .and_then(Value::as_object)
        .and_then(|map| map.keys().next().cloned())
}

fn collect_activitypub_audiences(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(uri)) => vec![uri.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .flat_map(|value| collect_activitypub_audiences(Some(value)))
            .collect(),
        Some(Value::Object(map)) => map
            .get("id")
            .and_then(Value::as_str)
            .map(|uri| vec![uri.to_owned()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
