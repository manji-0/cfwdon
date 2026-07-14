use cfwdon_domain::{QuoteApprovalPolicy, QuoteState, Visibility};
use stateright::{Checker, Model, Property};

/// Model-checker view of quote approval policy and quote-state resolution.
#[derive(Clone, Copy, Debug)]
pub(crate) struct QuoteModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct QuoteModelState {
    pub(crate) quote_state: QuoteState,
    pub(crate) visibility: Visibility,
    pub(crate) policy: QuoteApprovalPolicy,
    pub(crate) has_quote: bool,
    pub(crate) target_exists_locally: bool,
    pub(crate) blocked_by_owner: bool,
    pub(crate) is_owner: bool,
    pub(crate) is_follower: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuoteAction {
    ResolveInitial,
    ResolveRemote,
    ApplyVisibilityPolicy,
    Revoke,
}

const ACCOUNT_DEFAULT_POLICY: QuoteApprovalPolicy = QuoteApprovalPolicy::Followers;

/// Shared transition used by the Stateright model and worker refinement checks.
pub(crate) fn apply_quote_action(state: &mut QuoteModelState, action: QuoteAction) {
    match action {
        QuoteAction::ResolveInitial => {
            state.quote_state =
                QuoteState::initial_for_quote_target(state.has_quote, state.target_exists_locally);
        }
        QuoteAction::ResolveRemote => {
            let policy_allows = state.policy.allows_quote(state.is_owner, state.is_follower);
            state.quote_state =
                QuoteState::remote_for_target(state.blocked_by_owner, policy_allows);
        }
        QuoteAction::ApplyVisibilityPolicy => {
            state.policy = QuoteApprovalPolicy::for_status_visibility(
                state.visibility,
                None,
                ACCOUNT_DEFAULT_POLICY,
            );
        }
        QuoteAction::Revoke => {
            state.quote_state = QuoteState::quote_state_after_revoke(state.quote_state);
        }
    }
}

impl Model for QuoteModel {
    type State = QuoteModelState;
    type Action = QuoteAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();

        for visibility in [
            Visibility::Public,
            Visibility::Unlisted,
            Visibility::FollowersOnly,
            Visibility::Direct,
        ] {
            for has_quote in [false, true] {
                for target_exists_locally in [false, true] {
                    for blocked_by_owner in [false, true] {
                        for is_follower in [false, true] {
                            states.push(QuoteModelState {
                                quote_state: QuoteState::Pending,
                                visibility,
                                policy: QuoteApprovalPolicy::for_status_visibility(
                                    visibility,
                                    None,
                                    ACCOUNT_DEFAULT_POLICY,
                                ),
                                has_quote,
                                target_exists_locally,
                                blocked_by_owner,
                                is_owner: false,
                                is_follower,
                            });
                        }
                    }
                }
            }
        }

        states
    }

    fn actions(&self, _state: &Self::State, actions: &mut Vec<Self::Action>) {
        actions.extend([
            QuoteAction::ResolveInitial,
            QuoteAction::ResolveRemote,
            QuoteAction::ApplyVisibilityPolicy,
            QuoteAction::Revoke,
        ]);
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;
        apply_quote_action(&mut next, action);
        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "restricted_visibility_policy_nobody",
                |_, state: &QuoteModelState| {
                    !state.visibility.is_restricted() || state.policy == QuoteApprovalPolicy::Nobody
                },
            ),
            Property::always("revoked_not_visible", |_, state: &QuoteModelState| {
                state.quote_state != QuoteState::Revoked || !state.quote_state.is_visible()
            }),
            Property::always(
                "blocked_remote_is_rejected",
                |_, state: &QuoteModelState| {
                    !state.blocked_by_owner
                        || QuoteState::remote_for_target(
                            true,
                            state.policy.allows_quote(state.is_owner, state.is_follower),
                        ) == QuoteState::Rejected
                },
            ),
            Property::sometimes("accepted_reachable", |_, state: &QuoteModelState| {
                state.quote_state == QuoteState::Accepted
            }),
            Property::sometimes("pending_reachable", |_, state: &QuoteModelState| {
                state.quote_state == QuoteState::Pending
            }),
            Property::sometimes("rejected_reachable", |_, state: &QuoteModelState| {
                state.quote_state == QuoteState::Rejected
            }),
        ]
    }
}

pub(crate) fn check_quote_model() {
    QuoteModel.checker().spawn_dfs().join().assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_quote_model;

    #[test]
    fn quote_model_holds() {
        check_quote_model();
    }
}
