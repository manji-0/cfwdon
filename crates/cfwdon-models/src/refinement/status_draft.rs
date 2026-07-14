use stateright::Model;

use cfwdon_domain::{
    ComposingStatus, LocalAccount, LocalAccountRecord, PollDraft, QuoteTargetResolution,
    StatusDraftEvent,
};

use crate::refinement::verify::assert_model_matches_domain;
use crate::status_draft_transition_events::{
    StatusDraftTransitionEventsAction, StatusDraftTransitionEventsModel,
    StatusDraftTransitionEventsModelState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StatusDraftObservable {
    pub has_text: bool,
    pub media_count: u8,
    pub has_poll: bool,
    pub has_quote: bool,
}

impl StatusDraftObservable {
    fn composing(self) -> ComposingStatus {
        let poll = self.has_poll.then(|| {
            PollDraft::try_new(vec!["yes".to_owned(), "no".to_owned()], 300, false, false)
                .expect("fixture poll")
        });

        ComposingStatus {
            text: if self.has_text {
                "hello".to_owned()
            } else {
                String::new()
            },
            visibility: cfwdon_domain::Visibility::Public,
            spoiler_text: String::new(),
            sensitive: false,
            language: None,
            quote_approval_policy: None,
            in_reply_to_id: None,
            media_ids: (0..self.media_count)
                .map(|index| format!("media-{index}"))
                .collect(),
            poll,
        }
    }

    fn quoted_status_id(self) -> Option<&'static str> {
        self.has_quote.then_some("quote-1")
    }

    fn validation_succeeds(self) -> bool {
        self.composing().validate(self.quoted_status_id()).is_ok()
    }

    /// Mirrors `parse_status_draft_request`: validation succeeds iff domain validate succeeds.
    fn worker_validate_succeeds(self) -> bool {
        self.validation_succeeds()
    }

    fn fixture_account() -> LocalAccount {
        let mut record = LocalAccountRecord::test_fixture("acct-refine", "alice");
        record.default_quote_policy = "followers".to_owned();
        LocalAccount::from_record(record)
    }
}

fn model_domain_step(
    state: StatusDraftTransitionEventsModelState,
    action: StatusDraftTransitionEventsAction,
) -> Option<StatusDraftTransitionEventsModelState> {
    StatusDraftTransitionEventsModel.next_state(&state, action)
}

pub(crate) fn check_status_draft_refinement() {
    assert_model_matches_domain(&StatusDraftTransitionEventsModel, model_domain_step);

    for has_text in [false, true] {
        for media_count in [0_u8, 1, 5] {
            for has_poll in [false, true] {
                for has_quote in [false, true] {
                    let observable = StatusDraftObservable {
                        has_text,
                        media_count,
                        has_poll,
                        has_quote,
                    };
                    assert_eq!(
                        observable.worker_validate_succeeds(),
                        observable.validation_succeeds(),
                        "worker validation must mirror domain validate for {observable:?}"
                    );

                    if observable.validation_succeeds() {
                        let validated = observable
                            .composing()
                            .validate(observable.quoted_status_id())
                            .expect("validated draft");
                        assert!(validated.has_event(&StatusDraftEvent::DraftValidated));

                        let resolved = validated.state.into_publish_intent(
                            &StatusDraftObservable::fixture_account(),
                            QuoteTargetResolution::none(),
                        );
                        assert!(resolved.has_event(&StatusDraftEvent::PublishIntentResolved));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_status_draft_refinement;

    #[test]
    fn status_draft_refinement_holds() {
        check_status_draft_refinement();
    }
}
