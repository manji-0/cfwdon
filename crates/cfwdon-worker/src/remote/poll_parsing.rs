#[derive(Debug)]
pub(crate) struct RemotePollDraft {
    pub(crate) multiple: bool,
    pub(crate) expires_at: Option<String>,
    pub(crate) voters_count: Option<u64>,
    pub(crate) votes_count: u64,
    pub(crate) expired: bool,
    pub(crate) options: Vec<RemotePollOptionDraft>,
}

#[derive(Debug)]
pub(crate) struct RemotePollOptionDraft {
    pub(crate) title: String,
    pub(crate) votes_count: u64,
}

pub(crate) fn extract_remote_poll_draft(object: &serde_json::Value) -> Option<RemotePollDraft> {
    let (multiple, entries) =
        if let Some(entries) = object.get("anyOf").and_then(serde_json::Value::as_array) {
            (true, entries)
        } else if let Some(entries) = object.get("oneOf").and_then(serde_json::Value::as_array) {
            (false, entries)
        } else {
            return None;
        };

    let options = entries
        .iter()
        .filter_map(extract_remote_poll_option_draft)
        .collect::<Vec<_>>();
    if options.len() < 2 {
        return None;
    }

    let votes_count = options.iter().map(|option| option.votes_count).sum::<u64>();
    let expires_at = object
        .get("closed")
        .and_then(serde_json::Value::as_str)
        .or_else(|| object.get("endTime").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)?;

    Some(RemotePollDraft {
        multiple,
        expires_at: Some(expires_at.clone()),
        voters_count: object
            .get("votersCount")
            .and_then(serde_json::Value::as_u64),
        votes_count,
        expired: object.get("closed").is_some(),
        options,
    })
}

fn extract_remote_poll_option_draft(value: &serde_json::Value) -> Option<RemotePollOptionDraft> {
    let title = value
        .get("name")
        .and_then(serde_json::Value::as_str)?
        .trim()
        .to_owned();
    if title.is_empty() {
        return None;
    }

    Some(RemotePollOptionDraft {
        title,
        votes_count: value
            .pointer("/replies/totalItems")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    })
}
