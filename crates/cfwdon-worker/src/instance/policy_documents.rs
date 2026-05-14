pub(crate) fn configured_html_document(
    content: Option<&str>,
    metadata: Option<&str>,
    default_metadata: &str,
    is_terms: bool,
) -> Option<serde_json::Value> {
    let content = content?.trim();
    if content.is_empty() {
        return None;
    }

    let metadata = metadata
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_metadata);
    Some(if is_terms {
        serde_json::json!({
            "effective_date": metadata,
            "effective": true,
            "content": content,
            "succeeded_by": serde_json::Value::Null,
        })
    } else {
        serde_json::json!({
            "updated_at": metadata,
            "content": content,
        })
    })
}
