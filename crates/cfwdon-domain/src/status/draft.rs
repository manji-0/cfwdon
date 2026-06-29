use crate::account::LocalAccount;
use crate::error::StatusDraftError;
use crate::ids::{MediaId, StatusId};
use crate::quote::{QuoteApprovalPolicy, QuoteState};
use crate::status::poll::PollDraft;
use crate::status::visibility::Visibility;
use crate::transition::Transition;

/// Raw status composition input before domain validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposingStatus {
    pub text: String,
    pub visibility: Visibility,
    pub spoiler_text: String,
    pub sensitive: bool,
    pub language: Option<String>,
    pub quote_approval_policy: Option<QuoteApprovalPolicy>,
    pub in_reply_to_id: Option<String>,
    pub media_ids: Vec<String>,
    pub poll: Option<PollDraft>,
}

impl ComposingStatus {
    pub fn validate(
        self,
        quoted_status_id: Option<&str>,
    ) -> Result<Transition<StatusDraft, ()>, StatusDraftError> {
        let has_poll = self.poll.is_some();
        let media_ids = self
            .media_ids
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();

        if self.text.trim().is_empty() && media_ids.is_empty() && !has_poll {
            return Err(StatusDraftError::EmptyPayload);
        }
        if media_ids.len() > 4 {
            return Err(StatusDraftError::TooManyMedia);
        }
        if has_poll && !media_ids.is_empty() {
            return Err(StatusDraftError::PollWithMedia);
        }
        if quoted_status_id.is_some() && (has_poll || !media_ids.is_empty()) {
            return Err(StatusDraftError::QuoteWithMediaOrPoll);
        }

        Ok(Transition::without_events(StatusDraft {
            text: self.text.trim().to_owned(),
            visibility: self.visibility,
            spoiler_text: self.spoiler_text.trim().to_owned(),
            sensitive: self.sensitive,
            language: self
                .language
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty()),
            quote_approval_policy: self.quote_approval_policy,
            in_reply_to_id: self
                .in_reply_to_id
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
            media_ids,
            poll: self.poll,
        }))
    }
}

/// Validated local status composition ready for scheduling or publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusDraft {
    pub text: String,
    pub visibility: Visibility,
    pub spoiler_text: String,
    pub sensitive: bool,
    pub language: Option<String>,
    pub quote_approval_policy: Option<QuoteApprovalPolicy>,
    pub in_reply_to_id: Option<String>,
    pub media_ids: Vec<String>,
    pub poll: Option<PollDraft>,
}

impl StatusDraft {
    pub fn effective_quote_policy(&self, account: &LocalAccount) -> QuoteApprovalPolicy {
        QuoteApprovalPolicy::for_status_visibility(
            self.visibility,
            self.quote_approval_policy,
            account.resolved_default_quote_policy(),
        )
    }

    pub fn into_publish_intent(
        self,
        account: &LocalAccount,
        quote_target: QuoteTargetResolution,
    ) -> PublishIntent {
        PublishIntent {
            quote_policy: self.effective_quote_policy(account),
            quote_state: quote_target.initial_state(),
            draft: self,
        }
    }
}

/// Facts about a quote target resolved outside the domain boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteTargetResolution {
    pub has_quote: bool,
    pub target_exists_locally: bool,
}

impl QuoteTargetResolution {
    pub fn none() -> Self {
        Self {
            has_quote: false,
            target_exists_locally: false,
        }
    }

    pub fn with_target(target_exists_locally: bool) -> Self {
        Self {
            has_quote: true,
            target_exists_locally,
        }
    }

    pub fn initial_state(self) -> QuoteState {
        QuoteState::initial_for_quote_target(self.has_quote, self.target_exists_locally)
    }
}

/// Publication-ready status intent with resolved quote metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishIntent {
    pub draft: StatusDraft,
    pub quote_policy: QuoteApprovalPolicy,
    pub quote_state: QuoteState,
}

impl PublishIntent {
    pub fn in_reply_to_id(&self) -> Option<StatusId> {
        self.draft
            .in_reply_to_id
            .as_deref()
            .map(StatusId::new)
            .transpose()
            .ok()
            .flatten()
    }

    pub fn media_ids(&self) -> impl Iterator<Item = Result<MediaId, crate::error::IdError>> + '_ {
        self.draft
            .media_ids
            .iter()
            .map(|value| MediaId::new(value.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::account::{LocalAccount, LocalAccountRecord};

    fn fixture_account() -> LocalAccount {
        let mut record = LocalAccountRecord::test_fixture("acct-1", "alice");
        record.default_quote_policy = "followers".to_owned();
        LocalAccount::from_record(record)
    }

    #[test]
    fn composing_status_rejects_empty_payload() {
        let composing = ComposingStatus {
            text: String::new(),
            visibility: Visibility::Public,
            spoiler_text: String::new(),
            sensitive: false,
            language: None,
            quote_approval_policy: None,
            in_reply_to_id: None,
            media_ids: Vec::new(),
            poll: None,
        };
        assert_eq!(
            composing.validate(None).unwrap_err(),
            StatusDraftError::EmptyPayload
        );
    }

    #[test]
    fn publish_intent_resolves_quote_policy_from_account_default() {
        let draft = ComposingStatus {
            text: "hello".to_owned(),
            visibility: Visibility::Public,
            spoiler_text: String::new(),
            sensitive: false,
            language: None,
            quote_approval_policy: None,
            in_reply_to_id: None,
            media_ids: Vec::new(),
            poll: None,
        }
        .validate(None)
        .expect("valid draft")
        .state;
        let intent = draft.into_publish_intent(&fixture_account(), QuoteTargetResolution::none());

        assert_eq!(intent.quote_policy, QuoteApprovalPolicy::Followers);
        assert_eq!(intent.quote_state, QuoteState::Accepted);
    }
}
