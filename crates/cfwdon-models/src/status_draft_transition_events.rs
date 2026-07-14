use cfwdon_domain::{
    ComposingStatus, LocalAccount, LocalAccountRecord, PollDraft, QuoteTargetResolution,
    StatusDraftError, StatusDraftEvent, Visibility,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct StatusDraftTransitionEventsModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StatusDraftEventStage {
    Composing,
    DraftValidated,
    PublishIntentResolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StatusDraftTransitionEventsModelState {
    stage: StatusDraftEventStage,
    has_text: bool,
    media_count: u8,
    has_poll: bool,
    has_quote: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum StatusDraftTransitionEventsAction {
    ToggleText,
    AddMedia,
    RemoveMedia,
    TogglePoll,
    ToggleQuote,
    Validate,
    ResolvePublishIntent,
}

impl StatusDraftTransitionEventsModel {
    fn fixture_account() -> LocalAccount {
        let mut record = LocalAccountRecord::test_fixture("acct-event", "alice");
        record.default_quote_policy = "followers".to_owned();
        LocalAccount::from_record(record)
    }

    fn composing_status(state: &StatusDraftTransitionEventsModelState) -> ComposingStatus {
        let poll = state.has_poll.then(|| {
            PollDraft::try_new(vec!["yes".to_owned(), "no".to_owned()], 300, false, false)
                .expect("fixture poll")
        });

        ComposingStatus {
            text: if state.has_text {
                "hello".to_owned()
            } else {
                String::new()
            },
            visibility: Visibility::Public,
            spoiler_text: String::new(),
            sensitive: false,
            language: None,
            quote_approval_policy: None,
            in_reply_to_id: None,
            media_ids: (0..state.media_count)
                .map(|index| format!("media-{index}"))
                .collect(),
            poll,
        }
    }

    fn quoted_status_id(state: &StatusDraftTransitionEventsModelState) -> Option<&'static str> {
        state.has_quote.then_some("quote-1")
    }

    fn validation_succeeds(state: &StatusDraftTransitionEventsModelState) -> bool {
        Self::composing_status(state)
            .validate(Self::quoted_status_id(state))
            .is_ok()
    }
}

impl Model for StatusDraftTransitionEventsModel {
    type State = StatusDraftTransitionEventsModelState;
    type Action = StatusDraftTransitionEventsAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![StatusDraftTransitionEventsModelState {
            stage: StatusDraftEventStage::Composing,
            has_text: false,
            media_count: 0,
            has_poll: false,
            has_quote: false,
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        match state.stage {
            StatusDraftEventStage::Composing => {
                actions.push(StatusDraftTransitionEventsAction::ToggleText);
                if state.media_count < 5 {
                    actions.push(StatusDraftTransitionEventsAction::AddMedia);
                }
                if state.media_count > 0 {
                    actions.push(StatusDraftTransitionEventsAction::RemoveMedia);
                }
                actions.push(StatusDraftTransitionEventsAction::TogglePoll);
                actions.push(StatusDraftTransitionEventsAction::ToggleQuote);
                actions.push(StatusDraftTransitionEventsAction::Validate);
            }
            StatusDraftEventStage::DraftValidated => {
                actions.push(StatusDraftTransitionEventsAction::ResolvePublishIntent);
            }
            StatusDraftEventStage::PublishIntentResolved => {}
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            StatusDraftTransitionEventsAction::ToggleText => {
                if state.stage != StatusDraftEventStage::Composing {
                    return None;
                }
                next.has_text = !next.has_text;
            }
            StatusDraftTransitionEventsAction::AddMedia => {
                if state.stage != StatusDraftEventStage::Composing || state.media_count >= 5 {
                    return None;
                }
                next.media_count += 1;
            }
            StatusDraftTransitionEventsAction::RemoveMedia => {
                if state.stage != StatusDraftEventStage::Composing || state.media_count == 0 {
                    return None;
                }
                next.media_count -= 1;
            }
            StatusDraftTransitionEventsAction::TogglePoll => {
                if state.stage != StatusDraftEventStage::Composing {
                    return None;
                }
                next.has_poll = !next.has_poll;
            }
            StatusDraftTransitionEventsAction::ToggleQuote => {
                if state.stage != StatusDraftEventStage::Composing {
                    return None;
                }
                next.has_quote = !next.has_quote;
            }
            StatusDraftTransitionEventsAction::Validate => {
                if state.stage != StatusDraftEventStage::Composing
                    || !Self::validation_succeeds(state)
                {
                    return None;
                }
                next.stage = StatusDraftEventStage::DraftValidated;
            }
            StatusDraftTransitionEventsAction::ResolvePublishIntent => {
                if state.stage != StatusDraftEventStage::DraftValidated {
                    return None;
                }
                next.stage = StatusDraftEventStage::PublishIntentResolved;
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "validate_emits_draft_validated_event",
                |_, state: &StatusDraftTransitionEventsModelState| {
                    state.stage != StatusDraftEventStage::DraftValidated
                        || Self::composing_status(state)
                            .validate(Self::quoted_status_id(state))
                            .map(|transition| {
                                transition.has_event(&StatusDraftEvent::DraftValidated)
                            })
                            .unwrap_or(false)
                },
            ),
            Property::always(
                "publish_intent_emits_publish_intent_resolved_event",
                |_, state: &StatusDraftTransitionEventsModelState| {
                    state.stage != StatusDraftEventStage::PublishIntentResolved || {
                        let validated = Self::composing_status(state)
                            .validate(Self::quoted_status_id(state))
                            .expect("validated draft");
                        let resolved = validated.state.into_publish_intent(
                            &Self::fixture_account(),
                            QuoteTargetResolution::none(),
                        );
                        resolved.has_event(&StatusDraftEvent::PublishIntentResolved)
                    }
                },
            ),
            Property::always(
                "failed_validation_emits_no_domain_transition",
                |_, state: &StatusDraftTransitionEventsModelState| {
                    Self::validation_succeeds(state)
                        || matches!(
                            Self::composing_status(state).validate(Self::quoted_status_id(state)),
                            Err(StatusDraftError::EmptyPayload)
                                | Err(StatusDraftError::TooManyMedia)
                                | Err(StatusDraftError::PollWithMedia)
                                | Err(StatusDraftError::QuoteWithMediaOrPoll)
                        )
                },
            ),
            Property::sometimes(
                "publish_intent_resolved_event_reachable",
                |_, state: &StatusDraftTransitionEventsModelState| {
                    state.stage == StatusDraftEventStage::PublishIntentResolved
                },
            ),
        ]
    }
}

pub(crate) fn check_status_draft_transition_events_model() {
    StatusDraftTransitionEventsModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_status_draft_transition_events_model;

    #[test]
    fn status_draft_transition_events_model_holds() {
        check_status_draft_transition_events_model();
    }
}
