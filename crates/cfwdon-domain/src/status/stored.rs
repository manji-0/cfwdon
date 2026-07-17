use crate::quote::{QuoteApprovalPolicy, QuoteState};
use crate::status::record::LocalStatusRecord;
use crate::status::visibility::Visibility;
use crate::transition::Transition;

use super::draft::PublishIntent;

/// Worker-resolved facts required to persist a published local status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalStatusPersistenceFacts {
    pub status_id: String,
    pub account_id: String,
    pub ap_id: String,
    pub quote_of_uri: Option<String>,
    pub content_html: String,
    pub application_id: Option<i64>,
    pub created_at: String,
}

/// Persistence-ready local status after publication metadata is resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredLocalStatusIntent {
    pub status_id: String,
    pub account_id: String,
    pub ap_id: String,
    pub in_reply_to_id: Option<String>,
    pub quote_of_uri: Option<String>,
    pub content_html: String,
    pub text_content: String,
    pub spoiler_text: String,
    pub visibility: Visibility,
    pub sensitive: bool,
    pub language: Option<String>,
    pub quote_approval_policy: QuoteApprovalPolicy,
    pub quote_state: QuoteState,
    pub application_id: Option<i64>,
    pub created_at: String,
}

impl PublishIntent {
    pub fn into_stored_intent(
        self,
        facts: LocalStatusPersistenceFacts,
    ) -> Transition<StoredLocalStatusIntent, ()> {
        Transition::without_events(StoredLocalStatusIntent {
            status_id: facts.status_id,
            account_id: facts.account_id,
            ap_id: facts.ap_id,
            in_reply_to_id: self.draft.in_reply_to_id().map(str::to_owned),
            quote_of_uri: facts.quote_of_uri,
            content_html: facts.content_html,
            text_content: self.draft.text().to_owned(),
            spoiler_text: self.draft.spoiler_text().to_owned(),
            visibility: self.draft.visibility(),
            sensitive: self.draft.sensitive(),
            language: self.draft.language().map(str::to_owned),
            quote_approval_policy: self.quote_policy,
            quote_state: self.quote_state,
            application_id: facts.application_id,
            created_at: facts.created_at,
        })
    }
}

impl StoredLocalStatusIntent {
    pub fn to_record(&self) -> LocalStatusRecord {
        LocalStatusRecord {
            id: self.status_id.clone(),
            account_id: self.account_id.clone(),
            ap_id: Some(self.ap_id.clone()),
            in_reply_to_id: self.in_reply_to_id.clone(),
            boost_of_uri: None,
            quote_of_uri: self.quote_of_uri.clone(),
            content_html: self.content_html.clone(),
            _text_content: self.text_content.clone(),
            spoiler_text: self.spoiler_text.clone(),
            visibility: self.visibility.as_str().to_owned(),
            sensitive: i32::from(self.sensitive),
            language: self.language.clone(),
            quote_approval_policy: Some(self.quote_approval_policy.as_str().to_owned()),
            quote_state: self.quote_state.as_str().to_owned(),
            application_id: self.application_id,
            created_at: self.created_at.clone(),
            updated_at: Some(self.created_at.clone()),
        }
    }
}

/// Worker-resolved facts required to persist a local reblog wrapper status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalReblogPersistenceFacts {
    pub status_id: String,
    pub account_id: String,
    pub ap_id: String,
    pub boost_of_uri: String,
    pub visibility: Visibility,
    pub created_at: String,
}

/// Persistence-ready local reblog wrapper after metadata is resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredLocalReblogIntent {
    pub status_id: String,
    pub account_id: String,
    pub ap_id: String,
    pub boost_of_uri: String,
    pub visibility: Visibility,
    pub created_at: String,
}

impl StoredLocalReblogIntent {
    pub fn new(facts: LocalReblogPersistenceFacts) -> Self {
        Self {
            status_id: facts.status_id,
            account_id: facts.account_id,
            ap_id: facts.ap_id,
            boost_of_uri: facts.boost_of_uri,
            visibility: facts.visibility,
            created_at: facts.created_at,
        }
    }

    pub fn to_record(&self) -> LocalStatusRecord {
        LocalStatusRecord {
            id: self.status_id.clone(),
            account_id: self.account_id.clone(),
            ap_id: Some(self.ap_id.clone()),
            in_reply_to_id: None,
            boost_of_uri: Some(self.boost_of_uri.clone()),
            quote_of_uri: None,
            content_html: String::new(),
            _text_content: String::new(),
            spoiler_text: String::new(),
            visibility: self.visibility.as_str().to_owned(),
            sensitive: 0,
            language: None,
            quote_approval_policy: Some(QuoteApprovalPolicy::Public.as_str().to_owned()),
            quote_state: QuoteState::Accepted.as_str().to_owned(),
            application_id: None,
            created_at: self.created_at.clone(),
            updated_at: Some(self.created_at.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComposingStatus;
    use crate::QuoteTargetResolution;
    use crate::account::{LocalAccount, LocalAccountRecord};

    fn fixture_account() -> LocalAccount {
        let mut record = LocalAccountRecord::test_fixture("acct-1", "alice");
        record.default_quote_policy = "followers".to_owned();
        LocalAccount::from_record(record)
    }

    #[test]
    fn publish_intent_into_stored_intent_maps_persistence_fields() {
        let draft = ComposingStatus {
            text: "hello".to_owned(),
            visibility: Visibility::Unlisted,
            spoiler_text: "cw".to_owned(),
            sensitive: true,
            language: Some("ja".to_owned()),
            quote_approval_policy: None,
            in_reply_to_id: Some("reply-1".to_owned()),
            media_ids: Vec::new(),
            poll: None,
        }
        .validate(None)
        .expect("valid draft")
        .state;
        let intent = draft
            .into_publish_intent(
                &fixture_account(),
                QuoteTargetResolution::with_target(false),
            )
            .state;
        let stored = intent
            .into_stored_intent(LocalStatusPersistenceFacts {
                status_id: "status-1".to_owned(),
                account_id: "acct-1".to_owned(),
                ap_id: "https://social.example/users/alice/statuses/status-1".to_owned(),
                quote_of_uri: Some("https://remote.example/statuses/quote-1".to_owned()),
                content_html: "<p>hello</p>".to_owned(),
                application_id: Some(42),
                created_at: "2026-01-02T03:04:05.000Z".to_owned(),
            })
            .state;

        assert_eq!(stored.status_id, "status-1");
        assert_eq!(stored.text_content, "hello");
        assert_eq!(stored.visibility, Visibility::Unlisted);
        assert_eq!(stored.quote_approval_policy, QuoteApprovalPolicy::Followers);
        assert_eq!(stored.quote_state, QuoteState::Pending);
        assert_eq!(
            stored.to_record().quote_of_uri.as_deref(),
            Some("https://remote.example/statuses/quote-1")
        );
    }

    #[test]
    fn stored_local_reblog_intent_maps_wrapper_row() {
        let intent = StoredLocalReblogIntent::new(LocalReblogPersistenceFacts {
            status_id: "boost-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: "https://social.example/users/alice/statuses/boost-1".to_owned(),
            boost_of_uri: "https://remote.example/statuses/target-1".to_owned(),
            visibility: Visibility::Unlisted,
            created_at: "2026-01-02T03:04:05.000Z".to_owned(),
        });
        let record = intent.to_record();

        assert_eq!(
            record.boost_of_uri.as_deref(),
            Some("https://remote.example/statuses/target-1")
        );
        assert_eq!(record.visibility, "unlisted");
        assert!(record._text_content.is_empty());
    }
}
