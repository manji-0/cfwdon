use cfwdon_domain::{
    ComposingStatus, LocalAccount, LocalAccountRecord, PollDraft, QuoteApprovalPolicy, QuoteState,
    QuoteTargetResolution, StatusDraftError, Visibility,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct StatusDraftPublishModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PublishStage {
    Composing,
    Validated,
    Published,
    ValidationFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StatusDraftPublishModelState {
    stage: PublishStage,
    has_text: bool,
    media_count: u8,
    has_poll: bool,
    has_quote: bool,
    visibility: Visibility,
    policy_override: Option<QuoteApprovalPolicy>,
    account_default: QuoteApprovalPolicy,
    quote_target_local: bool,
    published_policy: Option<QuoteApprovalPolicy>,
    published_quote_state: Option<QuoteState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum StatusDraftPublishAction {
    ToggleText,
    AddMedia,
    RemoveMedia,
    TogglePoll,
    ToggleQuote,
    CycleVisibility,
    CyclePolicyOverride,
    CycleAccountDefault,
    ToggleQuoteTargetLocal,
    Validate,
    Publish,
}

impl StatusDraftPublishModel {
    fn fixture_account(account_default: QuoteApprovalPolicy) -> LocalAccount {
        let mut record = LocalAccountRecord::test_fixture("acct-model", "alice");
        record.default_quote_policy = account_default.as_str().to_owned();
        LocalAccount::from_record(record)
    }

    fn composing_status(state: &StatusDraftPublishModelState) -> ComposingStatus {
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
            visibility: state.visibility,
            spoiler_text: String::new(),
            sensitive: false,
            language: None,
            quote_approval_policy: state.policy_override,
            in_reply_to_id: None,
            media_ids: (0..state.media_count)
                .map(|index| format!("media-{index}"))
                .collect(),
            poll,
        }
    }

    fn quote_resolution(state: &StatusDraftPublishModelState) -> QuoteTargetResolution {
        if state.has_quote {
            QuoteTargetResolution::with_target(state.quote_target_local)
        } else {
            QuoteTargetResolution::none()
        }
    }

    fn expected_validation_error(state: &StatusDraftPublishModelState) -> Option<StatusDraftError> {
        if !state.has_text && state.media_count == 0 && !state.has_poll {
            return Some(StatusDraftError::EmptyPayload);
        }
        if state.media_count > 4 {
            return Some(StatusDraftError::TooManyMedia);
        }
        if state.has_poll && state.media_count > 0 {
            return Some(StatusDraftError::PollWithMedia);
        }
        if state.has_quote && (state.has_poll || state.media_count > 0) {
            return Some(StatusDraftError::QuoteWithMediaOrPoll);
        }
        None
    }

    fn cycle_visibility(visibility: Visibility) -> Visibility {
        match visibility {
            Visibility::Public => Visibility::Unlisted,
            Visibility::Unlisted => Visibility::FollowersOnly,
            Visibility::FollowersOnly => Visibility::Direct,
            Visibility::Direct => Visibility::Public,
        }
    }

    fn cycle_policy_override(current: Option<QuoteApprovalPolicy>) -> Option<QuoteApprovalPolicy> {
        match current {
            None => Some(QuoteApprovalPolicy::Public),
            Some(QuoteApprovalPolicy::Public) => Some(QuoteApprovalPolicy::Followers),
            Some(QuoteApprovalPolicy::Followers) => Some(QuoteApprovalPolicy::Nobody),
            Some(QuoteApprovalPolicy::Nobody) => None,
        }
    }

    fn cycle_account_default(current: QuoteApprovalPolicy) -> QuoteApprovalPolicy {
        match current {
            QuoteApprovalPolicy::Public => QuoteApprovalPolicy::Followers,
            QuoteApprovalPolicy::Followers => QuoteApprovalPolicy::Nobody,
            QuoteApprovalPolicy::Nobody => QuoteApprovalPolicy::Public,
        }
    }
}

impl Model for StatusDraftPublishModel {
    type State = StatusDraftPublishModelState;
    type Action = StatusDraftPublishAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();

        for has_text in [false, true] {
            for media_count in [0_u8, 1, 5] {
                for has_poll in [false, true] {
                    for has_quote in [false, true] {
                        for visibility in [
                            Visibility::Public,
                            Visibility::Unlisted,
                            Visibility::FollowersOnly,
                            Visibility::Direct,
                        ] {
                            for quote_target_local in [false, true] {
                                states.push(StatusDraftPublishModelState {
                                    stage: PublishStage::Composing,
                                    has_text,
                                    media_count,
                                    has_poll,
                                    has_quote,
                                    visibility,
                                    policy_override: None,
                                    account_default: QuoteApprovalPolicy::Followers,
                                    quote_target_local,
                                    published_policy: None,
                                    published_quote_state: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        states
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        match state.stage {
            PublishStage::Composing => {
                actions.push(StatusDraftPublishAction::ToggleText);
                if state.media_count < 5 {
                    actions.push(StatusDraftPublishAction::AddMedia);
                }
                if state.media_count > 0 {
                    actions.push(StatusDraftPublishAction::RemoveMedia);
                }
                actions.push(StatusDraftPublishAction::TogglePoll);
                actions.push(StatusDraftPublishAction::ToggleQuote);
                actions.push(StatusDraftPublishAction::CycleVisibility);
                actions.push(StatusDraftPublishAction::CyclePolicyOverride);
                actions.push(StatusDraftPublishAction::CycleAccountDefault);
                actions.push(StatusDraftPublishAction::ToggleQuoteTargetLocal);
                actions.push(StatusDraftPublishAction::Validate);
            }
            PublishStage::Validated => {
                actions.push(StatusDraftPublishAction::Publish);
            }
            PublishStage::Published | PublishStage::ValidationFailed => {}
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            StatusDraftPublishAction::ToggleText => {
                if state.stage != PublishStage::Composing {
                    return None;
                }
                next.has_text = !next.has_text;
            }
            StatusDraftPublishAction::AddMedia => {
                if state.stage != PublishStage::Composing || state.media_count >= 5 {
                    return None;
                }
                next.media_count += 1;
            }
            StatusDraftPublishAction::RemoveMedia => {
                if state.stage != PublishStage::Composing || state.media_count == 0 {
                    return None;
                }
                next.media_count -= 1;
            }
            StatusDraftPublishAction::TogglePoll => {
                if state.stage != PublishStage::Composing {
                    return None;
                }
                next.has_poll = !next.has_poll;
            }
            StatusDraftPublishAction::ToggleQuote => {
                if state.stage != PublishStage::Composing {
                    return None;
                }
                next.has_quote = !next.has_quote;
            }
            StatusDraftPublishAction::CycleVisibility => {
                if state.stage != PublishStage::Composing {
                    return None;
                }
                next.visibility = Self::cycle_visibility(next.visibility);
            }
            StatusDraftPublishAction::CyclePolicyOverride => {
                if state.stage != PublishStage::Composing {
                    return None;
                }
                next.policy_override = Self::cycle_policy_override(next.policy_override);
            }
            StatusDraftPublishAction::CycleAccountDefault => {
                if state.stage != PublishStage::Composing {
                    return None;
                }
                next.account_default = Self::cycle_account_default(next.account_default);
            }
            StatusDraftPublishAction::ToggleQuoteTargetLocal => {
                if state.stage != PublishStage::Composing {
                    return None;
                }
                next.quote_target_local = !next.quote_target_local;
            }
            StatusDraftPublishAction::Validate => {
                if state.stage != PublishStage::Composing {
                    return None;
                }
                let quoted_status_id = state.has_quote.then_some("quote-1");
                match Self::composing_status(state).validate(quoted_status_id) {
                    Ok(_) => {
                        next.stage = PublishStage::Validated;
                    }
                    Err(_) => {
                        next.stage = PublishStage::ValidationFailed;
                    }
                }
            }
            StatusDraftPublishAction::Publish => {
                if state.stage != PublishStage::Validated {
                    return None;
                }
                let quoted_status_id = state.has_quote.then_some("quote-1");
                let draft = Self::composing_status(state)
                    .validate(quoted_status_id)
                    .expect("validated draft")
                    .state;
                let account = Self::fixture_account(state.account_default);
                let intent = draft.into_publish_intent(&account, Self::quote_resolution(state));
                next.stage = PublishStage::Published;
                next.published_policy = Some(intent.quote_policy);
                next.published_quote_state = Some(intent.quote_state);
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "validation_failed_matches_domain_rules",
                |_, state: &StatusDraftPublishModelState| {
                    state.stage != PublishStage::ValidationFailed
                        || Self::expected_validation_error(state).is_some()
                },
            ),
            Property::always(
                "validated_compositions_have_no_validation_error",
                |_, state: &StatusDraftPublishModelState| {
                    state.stage != PublishStage::Validated
                        || Self::expected_validation_error(state).is_none()
                },
            ),
            Property::always(
                "published_implies_prior_validation",
                |_, state: &StatusDraftPublishModelState| {
                    state.stage != PublishStage::Published
                        || Self::expected_validation_error(state).is_none()
                },
            ),
            Property::always(
                "restricted_visibility_policy_nobody",
                |_, state: &StatusDraftPublishModelState| {
                    state.stage != PublishStage::Published
                        || !state.visibility.is_restricted()
                        || state.published_policy == Some(QuoteApprovalPolicy::Nobody)
                },
            ),
            Property::always(
                "published_quote_state_matches_target",
                |_, state: &StatusDraftPublishModelState| {
                    state.stage != PublishStage::Published
                        || state.published_quote_state
                            == Some(Self::quote_resolution(state).initial_state())
                },
            ),
            Property::always(
                "published_quote_policy_matches_effective",
                |_, state: &StatusDraftPublishModelState| {
                    if state.stage != PublishStage::Published {
                        return true;
                    }
                    let expected = QuoteApprovalPolicy::for_status_visibility(
                        state.visibility,
                        state.policy_override,
                        state.account_default,
                    );
                    state.published_policy == Some(expected)
                },
            ),
            Property::always(
                "terminal_stages_are_final",
                |_, state: &StatusDraftPublishModelState| {
                    !matches!(
                        state.stage,
                        PublishStage::Published | PublishStage::ValidationFailed
                    ) || state.stage == PublishStage::Published
                        || Self::expected_validation_error(state).is_some()
                },
            ),
            Property::sometimes(
                "valid_publish_reachable",
                |_, state: &StatusDraftPublishModelState| state.stage == PublishStage::Published,
            ),
            Property::sometimes(
                "validation_failure_reachable",
                |_, state: &StatusDraftPublishModelState| {
                    state.stage == PublishStage::ValidationFailed
                },
            ),
        ]
    }
}

pub(crate) fn check_status_draft_publish_model() {
    StatusDraftPublishModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_status_draft_publish_model;

    #[test]
    fn status_draft_publish_model_holds() {
        check_status_draft_publish_model();
    }
}
