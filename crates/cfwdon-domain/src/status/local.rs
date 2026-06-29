use crate::quote::{QuoteApprovalPolicy, QuoteState};
use crate::status::record::LocalStatusRecord;
use crate::status::visibility::Visibility;

/// Domain entity for a persisted local status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalStatus {
    pub id: String,
    pub account_id: String,
    pub ap_id: Option<String>,
    pub in_reply_to_id: Option<String>,
    pub boost_of_uri: Option<String>,
    pub quote_of_uri: Option<String>,
    pub content_html: String,
    pub text: String,
    pub spoiler_text: String,
    pub visibility: Visibility,
    pub sensitive: bool,
    pub language: Option<String>,
    pub quote_approval_policy: Option<QuoteApprovalPolicy>,
    pub quote_state: QuoteState,
    pub application_id: Option<i64>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

impl LocalStatus {
    pub fn from_record(record: LocalStatusRecord) -> Self {
        Self {
            id: record.id,
            account_id: record.account_id,
            ap_id: record.ap_id,
            in_reply_to_id: record.in_reply_to_id,
            boost_of_uri: record.boost_of_uri,
            quote_of_uri: record.quote_of_uri,
            content_html: record.content_html,
            text: record._text_content,
            spoiler_text: record.spoiler_text,
            visibility: Visibility::parse(&record.visibility).unwrap_or(Visibility::Public),
            sensitive: record.sensitive != 0,
            language: record.language,
            quote_approval_policy: record
                .quote_approval_policy
                .as_deref()
                .and_then(|value| QuoteApprovalPolicy::parse(value).ok()),
            quote_state: QuoteState::parse(&record.quote_state).unwrap_or(QuoteState::Accepted),
            application_id: record.application_id,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }

    pub fn to_record(&self) -> LocalStatusRecord {
        LocalStatusRecord {
            id: self.id.clone(),
            account_id: self.account_id.clone(),
            ap_id: self.ap_id.clone(),
            in_reply_to_id: self.in_reply_to_id.clone(),
            boost_of_uri: self.boost_of_uri.clone(),
            quote_of_uri: self.quote_of_uri.clone(),
            content_html: self.content_html.clone(),
            _text_content: self.text.clone(),
            spoiler_text: self.spoiler_text.clone(),
            visibility: self.visibility.as_str().to_owned(),
            sensitive: i32::from(self.sensitive),
            language: self.language.clone(),
            quote_approval_policy: self
                .quote_approval_policy
                .map(|policy| policy.as_str().to_owned()),
            quote_state: self.quote_state.as_str().to_owned(),
            application_id: self.application_id,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }

    pub fn effective_quote_state(&self) -> QuoteState {
        QuoteState::effective_for_stored(self.quote_of_uri.as_deref(), self.quote_state)
    }

    pub fn has_active_quote(&self) -> bool {
        self.quote_of_uri.is_some() && self.effective_quote_state().is_visible()
    }

    pub fn effective_quote_approval_policy(&self) -> QuoteApprovalPolicy {
        QuoteApprovalPolicy::for_stored_visibility(
            self.visibility.as_str(),
            self.quote_approval_policy.map(|policy| policy.as_str()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_status_record_roundtrip_preserves_entity() {
        let status = LocalStatus {
            id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: Some("https://example.com/users/alice/statuses/1".to_owned()),
            in_reply_to_id: None,
            boost_of_uri: None,
            quote_of_uri: Some("https://example.com/status/2".to_owned()),
            content_html: "<p>hello</p>".to_owned(),
            text: "hello".to_owned(),
            spoiler_text: String::new(),
            visibility: Visibility::Unlisted,
            sensitive: true,
            language: Some("en".to_owned()),
            quote_approval_policy: Some(QuoteApprovalPolicy::Followers),
            quote_state: QuoteState::Pending,
            application_id: Some(7),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: Some("2026-01-02T00:00:00Z".to_owned()),
        };

        let restored = LocalStatus::from_record(status.to_record());
        assert_eq!(status, restored);
    }
}
