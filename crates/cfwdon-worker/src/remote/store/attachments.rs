use crate::{RemoteStatusAttachmentRow, now_iso_string};

pub(crate) fn remote_status_attachments_from_object(
    status_id: &str,
    object: &serde_json::Value,
) -> Vec<RemoteStatusAttachmentRow> {
    let Some(values) = object
        .get("attachment")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };

    values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let remote_url = attachment_uri(value)?;
            Some(RemoteStatusAttachmentRow {
                id: format!("{status_id}:remote:{index}"),
                status_id: status_id.to_owned(),
                remote_url: remote_url.clone(),
                preview_url: value.get("icon").and_then(attachment_uri),
                content_type: value
                    .get("mediaType")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("application/octet-stream")
                    .to_owned(),
                description: value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                blurhash: value
                    .get("blurhash")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                width: value
                    .get("width")
                    .and_then(serde_json::Value::as_u64)
                    .map(|value| value as u32),
                height: value
                    .get("height")
                    .and_then(serde_json::Value::as_u64)
                    .map(|value| value as u32),
                created_at: now_iso_string().unwrap_or_default(),
            })
        })
        .collect()
}

pub(super) fn attachment_uri(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(uri) => Some(uri.clone()),
        serde_json::Value::Object(map) => map
            .get("url")
            .and_then(|url| match url {
                serde_json::Value::String(uri) => Some(uri.clone()),
                serde_json::Value::Array(values) => values.iter().find_map(attachment_uri),
                serde_json::Value::Object(_) => {
                    crate::activity_object_id(Some(url)).map(str::to_owned)
                }
                _ => None,
            })
            .or_else(|| {
                map.get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            }),
        serde_json::Value::Array(values) => values.iter().find_map(attachment_uri),
        _ => None,
    }
}
