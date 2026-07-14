use cfwdon_domain::QuoteState;
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct QuoteApprovalModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct QuoteApprovalModelState {
    pub(crate) quote_state: QuoteState,
    pub(crate) has_quote: bool,
    pub(crate) target_exists_locally: bool,
    pub(crate) blocked_by_owner: bool,
    pub(crate) policy_allows: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuoteApprovalAction {
    RemoteUpsert,
    OwnerApprove,
    OwnerReject,
    Revoke,
}

impl QuoteApprovalModel {
    fn has_active_quote(state: &QuoteApprovalModelState) -> bool {
        state.has_quote && state.quote_state.is_visible()
    }

    fn remote_upsert_incoming(state: &QuoteApprovalModelState) -> QuoteState {
        QuoteState::quote_state_for_remote_publish(
            state.has_quote,
            state.target_exists_locally,
            state.blocked_by_owner,
            state.policy_allows,
        )
    }
}

impl Model for QuoteApprovalModel {
    type State = QuoteApprovalModelState;
    type Action = QuoteApprovalAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();

        for target_exists_locally in [false, true] {
            for blocked_by_owner in [false, true] {
                for policy_allows in [false, true] {
                    states.push(QuoteApprovalModelState {
                        quote_state: QuoteState::quote_state_for_local_publish(
                            true,
                            target_exists_locally,
                        ),
                        has_quote: true,
                        target_exists_locally,
                        blocked_by_owner,
                        policy_allows,
                    });

                    if target_exists_locally {
                        states.push(QuoteApprovalModelState {
                            quote_state: QuoteState::quote_state_for_remote_publish(
                                true,
                                true,
                                blocked_by_owner,
                                policy_allows,
                            ),
                            has_quote: true,
                            target_exists_locally: true,
                            blocked_by_owner,
                            policy_allows,
                        });
                    }
                }
            }
        }

        states
    }

    fn actions(&self, _state: &Self::State, actions: &mut Vec<Self::Action>) {
        actions.extend([
            QuoteApprovalAction::RemoteUpsert,
            QuoteApprovalAction::OwnerApprove,
            QuoteApprovalAction::OwnerReject,
            QuoteApprovalAction::Revoke,
        ]);
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            QuoteApprovalAction::RemoteUpsert => {
                let incoming = Self::remote_upsert_incoming(state);
                next.quote_state =
                    QuoteState::quote_state_after_remote_upsert(state.quote_state, incoming);
            }
            QuoteApprovalAction::OwnerApprove => {
                next.quote_state = QuoteState::quote_state_after_owner_approve(state.quote_state);
            }
            QuoteApprovalAction::OwnerReject => {
                next.quote_state = QuoteState::quote_state_after_owner_reject(state.quote_state);
            }
            QuoteApprovalAction::Revoke => {
                next.quote_state = QuoteState::quote_state_after_revoke(state.quote_state);
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "revoked_is_sticky_on_remote_upsert",
                |_, state: &QuoteApprovalModelState| {
                    state.quote_state != QuoteState::Revoked
                        || QuoteState::quote_state_after_remote_upsert(
                            QuoteState::Revoked,
                            QuoteApprovalModel::remote_upsert_incoming(state),
                        ) == QuoteState::Revoked
                },
            ),
            Property::always(
                "revoked_not_visible",
                |_, state: &QuoteApprovalModelState| {
                    state.quote_state != QuoteState::Revoked || !state.quote_state.is_visible()
                },
            ),
            Property::always(
                "revoked_is_not_active_quote",
                |_, state: &QuoteApprovalModelState| {
                    state.quote_state != QuoteState::Revoked
                        || !QuoteApprovalModel::has_active_quote(state)
                },
            ),
            Property::always(
                "accepted_counts_in_quote_timeline",
                |_, state: &QuoteApprovalModelState| {
                    !state.has_quote
                        || !state.quote_state.counts_in_accepted_quotes_timeline()
                        || state.quote_state == QuoteState::Accepted
                },
            ),
            Property::always(
                "pending_shows_placeholder",
                |_, state: &QuoteApprovalModelState| {
                    !state.quote_state.shows_quote_placeholder()
                        || state.quote_state == QuoteState::Pending
                },
            ),
            Property::always(
                "blocked_local_remote_publish_is_rejected",
                |_, state: &QuoteApprovalModelState| {
                    !state.target_exists_locally
                        || !state.blocked_by_owner
                        || QuoteState::quote_state_for_remote_publish(
                            true,
                            true,
                            true,
                            state.policy_allows,
                        ) == QuoteState::Rejected
                },
            ),
            Property::sometimes(
                "accepted_reachable",
                |_, state: &QuoteApprovalModelState| state.quote_state == QuoteState::Accepted,
            ),
            Property::sometimes(
                "rejected_reachable",
                |_, state: &QuoteApprovalModelState| state.quote_state == QuoteState::Rejected,
            ),
            Property::sometimes("revoked_reachable", |_, state: &QuoteApprovalModelState| {
                state.quote_state == QuoteState::Revoked
            }),
        ]
    }
}

pub(crate) fn check_quote_approval_model() {
    QuoteApprovalModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_quote_approval_model;

    #[test]
    fn quote_approval_model_holds() {
        check_quote_approval_model();
    }
}
