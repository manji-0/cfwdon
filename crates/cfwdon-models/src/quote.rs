use cfwdon_domain::{QuoteApprovalPolicy, QuoteState, Visibility};
use stateright::{Checker, Model, Property};

/// Model-checker view of quote approval policy and quote-state resolution.
#[derive(Clone, Copy, Debug)]
pub(crate) struct QuoteModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct QuoteModelState {
    quote_state: QuoteState,
    visibility: Visibility,
    policy: QuoteApprovalPolicy,
    has_quote: bool,
    target_exists_locally: bool,
    blocked_by_owner: bool,
    is_owner: bool,
    is_follower: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuoteAction {
    ResolveInitial,
    ResolveRemote,
    ApplyVisibilityPolicy,
    Revoke,
}

impl Model for QuoteModel {
    type State = QuoteModelState;
    type Action = QuoteAction;

    fn init_states(&self) -> Vec<Self::State> {
        let account_default = QuoteApprovalPolicy::Followers;
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
                                    account_default,
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
        let account_default = QuoteApprovalPolicy::Followers;
        let mut next = *state;

        match action {
            QuoteAction::ResolveInitial => {
                next.quote_state = QuoteState::initial_for_quote_target(
                    next.has_quote,
                    next.target_exists_locally,
                );
            }
            QuoteAction::ResolveRemote => {
                let policy_allows = next.policy.allows_quote(next.is_owner, next.is_follower);
                next.quote_state =
                    QuoteState::remote_for_target(next.blocked_by_owner, policy_allows);
            }
            QuoteAction::ApplyVisibilityPolicy => {
                next.policy = QuoteApprovalPolicy::for_status_visibility(
                    next.visibility,
                    None,
                    account_default,
                );
            }
            QuoteAction::Revoke => {
                next.quote_state = QuoteState::Revoked;
            }
        }

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
