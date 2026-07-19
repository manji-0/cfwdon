use super::MastodonStatusResponse;

pub(crate) fn quote_document_with_state(
    state: &str,
    quoted_status: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "state": state,
        "quoted_status": quoted_status,
    })
}

pub(crate) fn pending_quote_document() -> serde_json::Value {
    quote_placeholder_document("pending")
}

pub(crate) fn quote_placeholder_document(state: &str) -> serde_json::Value {
    serde_json::json!({
        "state": state,
        "quoted_status": serde_json::Value::Null,
    })
}

pub(crate) fn unauthorized_quote_document() -> serde_json::Value {
    quote_placeholder_document("unauthorized")
}

pub(crate) fn quote_state_uses_placeholder(state: &str) -> bool {
    matches!(state, "revoked" | "rejected" | "unauthorized" | "deleted")
}

pub(crate) fn quote_document_for_local_state(
    local_quote_state: Option<&str>,
) -> Option<serde_json::Value> {
    match local_quote_state {
        Some("pending") => Some(pending_quote_document()),
        Some(state) if quote_state_uses_placeholder(state) => {
            Some(quote_placeholder_document(state))
        }
        _ => None,
    }
}

pub(crate) fn quote_document_from_response(
    state: &str,
    response: MastodonStatusResponse,
) -> serde_json::Value {
    quote_document_with_state(
        state,
        serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
    )
}

pub(crate) fn remote_quote_visibility_is_embeddable(visibility: &str) -> bool {
    matches!(visibility, "public" | "unlisted")
}

pub(crate) fn accepted_quote_document_state() -> &'static str {
    "accepted"
}

#[cfg(test)]
mod tests {
    use super::{
        accepted_quote_document_state, quote_state_uses_placeholder,
        remote_quote_visibility_is_embeddable, unauthorized_quote_document,
    };

    #[test]
    fn quote_state_uses_placeholder_for_terminal_states() {
        assert!(quote_state_uses_placeholder("revoked"));
        assert!(quote_state_uses_placeholder("rejected"));
        assert!(quote_state_uses_placeholder("unauthorized"));
        assert!(quote_state_uses_placeholder("deleted"));
        assert!(!quote_state_uses_placeholder("pending"));
        assert!(!quote_state_uses_placeholder("accepted"));
    }

    #[test]
    fn remote_quote_visibility_is_embeddable_for_public_timelines() {
        assert!(remote_quote_visibility_is_embeddable("public"));
        assert!(remote_quote_visibility_is_embeddable("unlisted"));
        assert!(!remote_quote_visibility_is_embeddable("private"));
        assert!(!remote_quote_visibility_is_embeddable("direct"));
    }

    #[test]
    fn accepted_quote_document_state_matches_mastodon_state_name() {
        assert_eq!(accepted_quote_document_state(), "accepted");
    }

    #[test]
    fn unauthorized_quote_document_uses_placeholder_shape() {
        let document = unauthorized_quote_document();

        assert_eq!(document["state"], serde_json::json!("unauthorized"));
        assert_eq!(document["quoted_status"], serde_json::Value::Null);
    }
}
