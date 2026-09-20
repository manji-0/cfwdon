use worker::Result;

pub(super) fn stream_hub_stream_labels(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) -> Vec<String> {
    let mut labels = vec![stream_name.to_owned()];
    if stream_name.starts_with("hashtag")
        && let Some(tag) = tag
    {
        labels.push(tag.to_owned());
    }
    if stream_name == "list"
        && let Some(list) = list
    {
        labels.push(list.to_owned());
    }
    labels
}

pub(crate) fn stream_hub_fanout_message(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
    event: &str,
    payload: &str,
) -> Result<String> {
    let mut message = serde_json::json!({
        "stream": stream_hub_stream_labels(stream_name, tag, list),
        "event": event,
    });
    if event != "filters_changed" {
        message["payload"] = serde_json::Value::String(payload.to_owned());
    }
    serde_json::to_string(&message).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize stream hub websocket event: {error}"
        ))
    })
}

pub(super) fn stream_hub_websocket_error_message(message: &str, status: u16) -> String {
    serde_json::json!({
        "error": message,
        "status": status,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_hub_fanout_message_matches_mastodon_shape() {
        let message =
            stream_hub_fanout_message("user:notification", None, None, "update", r#"{"id":"1"}"#)
                .unwrap();
        let parsed = serde_json::from_str::<serde_json::Value>(&message).unwrap();
        assert_eq!(parsed["stream"], serde_json::json!(["user:notification"]));
        assert_eq!(parsed["event"], "update");
        assert_eq!(parsed["payload"], r#"{"id":"1"}"#);
    }

    #[test]
    fn stream_hub_fanout_message_omits_payload_for_filters_changed() {
        let message =
            stream_hub_fanout_message("user", None, None, "filters_changed", "{}").unwrap();
        let parsed = serde_json::from_str::<serde_json::Value>(&message).unwrap();
        assert_eq!(parsed["stream"], serde_json::json!(["user"]));
        assert_eq!(parsed["event"], "filters_changed");
        assert!(parsed.get("payload").is_none());
    }

    #[test]
    fn stream_hub_fanout_message_includes_hashtag_and_list_labels() {
        let hashtag_message =
            stream_hub_fanout_message("hashtag:local", Some("rust"), None, "update", "{}").unwrap();
        let hashtag = serde_json::from_str::<serde_json::Value>(&hashtag_message).unwrap();
        assert_eq!(
            hashtag["stream"],
            serde_json::json!(["hashtag:local", "rust"])
        );

        let list_message =
            stream_hub_fanout_message("list", None, Some("list-1"), "update", "{}").unwrap();
        let list = serde_json::from_str::<serde_json::Value>(&list_message).unwrap();
        assert_eq!(list["stream"], serde_json::json!(["list", "list-1"]));
    }
}
