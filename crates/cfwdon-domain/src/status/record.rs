use crate::quote::{QuoteApprovalPolicy, QuoteState};
use crate::status::Visibility;
use serde::Deserialize;

/// Persistence-shaped local status row loaded from D1 or API adapters.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LocalStatusRecord {
    pub id: String,
    pub account_id: String,
    pub ap_id: Option<String>,
    pub in_reply_to_id: Option<String>,
    #[serde(default)]
    pub boost_of_uri: Option<String>,
    #[serde(default)]
    pub quote_of_uri: Option<String>,
    pub content_html: String,
    #[serde(rename = "text_content")]
    pub _text_content: String,
    pub spoiler_text: String,
    pub visibility: String,
    pub sensitive: i32,
    pub language: Option<String>,
    #[serde(default)]
    pub quote_approval_policy: Option<String>,
    #[serde(default = "default_quote_state")]
    pub quote_state: String,
    #[serde(default)]
    pub application_id: Option<i64>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_quote_state() -> String {
    QuoteState::Accepted.as_str().to_owned()
}

impl LocalStatusRecord {
    pub fn effective_quote_state(&self) -> QuoteState {
        let stored = QuoteState::parse(&self.quote_state).unwrap_or(QuoteState::Accepted);
        QuoteState::effective_for_stored(self.quote_of_uri.as_deref(), stored)
    }

    pub fn has_active_quote(&self) -> bool {
        self.quote_of_uri.is_some() && self.effective_quote_state().is_visible()
    }

    pub fn effective_quote_approval_policy(&self) -> QuoteApprovalPolicy {
        QuoteApprovalPolicy::for_stored_visibility(
            self.visibility.as_str(),
            self.quote_approval_policy.as_deref(),
        )
    }

    pub fn visibility_parsed(&self) -> Visibility {
        Visibility::parse(&self.visibility).unwrap_or(Visibility::Public)
    }

    pub fn is_sensitive(&self) -> bool {
        self.sensitive != 0
    }
}

pub fn local_status_default_quote_state() -> String {
    default_quote_state()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_record(quote_of_uri: Option<&str>, quote_state: &str) -> LocalStatusRecord {
        LocalStatusRecord {
            id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: Some("https://example.com/users/alice/statuses/1".to_owned()),
            in_reply_to_id: None,
            boost_of_uri: None,
            quote_of_uri: quote_of_uri.map(str::to_owned),
            content_html: "<p>hello</p>".to_owned(),
            _text_content: "hello".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 0,
            language: None,
            quote_approval_policy: None,
            quote_state: quote_state.to_owned(),
            application_id: None,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: None,
        }
    }

    #[test]
    fn effective_quote_state_defaults_to_accepted_without_quote() {
        let record = fixture_record(None, "pending");
        assert_eq!(record.effective_quote_state(), QuoteState::Accepted);
        assert!(!record.has_active_quote());
    }

    #[test]
    fn has_active_quote_depends_on_quote_state() {
        let mut record = fixture_record(Some("https://example.com/status/2"), "pending");
        assert_eq!(record.effective_quote_state(), QuoteState::Pending);
        assert!(record.has_active_quote());

        record.quote_state = "revoked".to_owned();
        assert_eq!(record.effective_quote_state(), QuoteState::Revoked);
        assert!(!record.has_active_quote());
    }

    #[test]
    fn effective_quote_approval_policy_forces_private_visibility_to_nobody() {
        let record = LocalStatusRecord {
            visibility: "private".to_owned(),
            quote_approval_policy: Some("public".to_owned()),
            ..fixture_record(None, "accepted")
        };
        assert_eq!(
            record.effective_quote_approval_policy(),
            QuoteApprovalPolicy::Nobody
        );
    }
}
