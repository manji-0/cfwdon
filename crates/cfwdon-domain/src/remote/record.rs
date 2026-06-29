use crate::quote::QuoteState;
use crate::status::Visibility;
use serde::Deserialize;

/// Persistence-shaped remote status row loaded from D1 or API adapters.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RemoteStatusRecord {
    pub id: String,
    pub actor_uri: String,
    pub object_uri: String,
    pub url: Option<String>,
    pub in_reply_to_uri: Option<String>,
    #[serde(default)]
    pub boost_of_uri: Option<String>,
    #[serde(default)]
    pub quote_of_uri: Option<String>,
    pub content_html: String,
    pub spoiler_text: String,
    pub visibility: String,
    pub sensitive: i32,
    pub language: Option<String>,
    #[serde(default = "default_quote_state")]
    pub quote_state: String,
    pub published_at: String,
}

fn default_quote_state() -> String {
    QuoteState::Accepted.as_str().to_owned()
}

impl RemoteStatusRecord {
    pub fn effective_quote_state(&self) -> QuoteState {
        let stored = QuoteState::parse(&self.quote_state).unwrap_or(QuoteState::Accepted);
        QuoteState::effective_for_stored(self.quote_of_uri.as_deref(), stored)
    }

    pub fn has_active_quote(&self) -> bool {
        self.quote_of_uri.is_some() && self.effective_quote_state().is_visible()
    }

    pub fn visibility_parsed(&self) -> Visibility {
        Visibility::parse(&self.visibility).unwrap_or(Visibility::Public)
    }

    pub fn is_sensitive(&self) -> bool {
        self.sensitive != 0
    }
}

pub fn remote_status_default_quote_state() -> String {
    default_quote_state()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_record(quote_of_uri: Option<&str>, quote_state: &str) -> RemoteStatusRecord {
        RemoteStatusRecord {
            id: "remote-1".to_owned(),
            actor_uri: "https://remote.example/users/bob".to_owned(),
            object_uri: "https://remote.example/users/bob/statuses/1".to_owned(),
            url: None,
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: quote_of_uri.map(str::to_owned),
            content_html: "<p>hello</p>".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 0,
            language: None,
            quote_state: quote_state.to_owned(),
            published_at: "2026-01-01T00:00:00Z".to_owned(),
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
        assert!(record.has_active_quote());

        record.quote_state = "revoked".to_owned();
        assert!(!record.has_active_quote());
    }
}
