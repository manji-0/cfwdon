use crate::render_status_html;

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

/// Prefer explicit HTML, otherwise wrap plain text for policy/description bodies.
pub(crate) fn policy_html_from_sources(
    html: Option<&str>,
    plain_text: Option<&str>,
) -> Option<String> {
    if let Some(html) = html.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(html.to_owned());
    }
    let plain = plain_text?.trim();
    if plain.is_empty() {
        return None;
    }
    Some(normalize_policy_body(plain))
}

pub(crate) fn normalize_policy_body(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.contains('<') {
        trimmed.to_owned()
    } else {
        render_status_html(trimmed)
    }
}

pub(crate) fn build_default_terms_of_service_document(content: &str) -> serde_json::Value {
    configured_html_document(
        Some(&normalize_policy_body(content)),
        None,
        "1970-01-01",
        true,
    )
    .unwrap_or_else(|| {
        serde_json::json!({
            "effective_date": "1970-01-01",
            "effective": true,
            "content": "",
            "succeeded_by": serde_json::Value::Null,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{normalize_policy_body, policy_html_from_sources};

    #[test]
    fn policy_html_from_sources_prefers_html_over_plain_text() {
        assert_eq!(
            policy_html_from_sources(Some("<p>html</p>"), Some("plain")),
            Some("<p>html</p>".to_owned())
        );
    }

    #[test]
    fn policy_html_from_sources_wraps_plain_text() {
        assert_eq!(
            policy_html_from_sources(None, Some("hello world")),
            Some("<p>hello world</p>".to_owned())
        );
        assert_eq!(
            normalize_policy_body("already <em>html</em>"),
            "already <em>html</em>"
        );
    }
}
