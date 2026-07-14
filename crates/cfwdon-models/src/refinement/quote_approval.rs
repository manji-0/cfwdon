use cfwdon_domain::{OwnerQuoteAction, QuoteState, merged_quote_state_for_remote_upsert};

use crate::quote_approval::{QuoteApprovalAction, QuoteApprovalModel, QuoteApprovalModelState};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};

/// Observable quote-approval facts persisted on a status row and used by worker handlers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct QuoteApprovalObservable {
    pub quote_state: QuoteState,
    pub has_quote: bool,
    pub target_exists_locally: bool,
    pub blocked_by_owner: bool,
    pub policy_allows: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuoteApprovalWorkerAction {
    RemoteUpsert,
    OwnerApprove,
    OwnerReject,
    Revoke,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct QuoteApprovalWorkerContext {
    pub observable: QuoteApprovalObservable,
    pub quote_of_uri_matches_target: bool,
    pub requester_is_quote_author: bool,
}

impl QuoteApprovalObservable {
    pub(crate) fn to_model_state(self) -> QuoteApprovalModelState {
        QuoteApprovalModelState {
            quote_state: self.quote_state,
            has_quote: self.has_quote,
            target_exists_locally: self.target_exists_locally,
            blocked_by_owner: self.blocked_by_owner,
            policy_allows: self.policy_allows,
        }
    }

    fn remote_upsert_incoming(self) -> QuoteState {
        QuoteState::quote_state_for_remote_publish(
            self.has_quote,
            self.target_exists_locally,
            self.blocked_by_owner,
            self.policy_allows,
        )
    }

    fn domain_step(self, action: QuoteApprovalWorkerAction) -> Self {
        let quote_state = match action {
            QuoteApprovalWorkerAction::RemoteUpsert => merged_quote_state_for_remote_upsert(
                self.quote_state,
                self.remote_upsert_incoming(),
            ),
            QuoteApprovalWorkerAction::OwnerApprove => {
                QuoteState::quote_state_after_owner_approve(self.quote_state)
            }
            QuoteApprovalWorkerAction::OwnerReject => {
                QuoteState::quote_state_after_owner_reject(self.quote_state)
            }
            QuoteApprovalWorkerAction::Revoke => {
                QuoteState::quote_state_after_revoke(self.quote_state)
            }
        };

        Self {
            quote_state,
            ..self
        }
    }
}

impl QuoteApprovalWorkerContext {
    pub(crate) fn worker_allows(self, action: QuoteApprovalWorkerAction) -> bool {
        match action {
            QuoteApprovalWorkerAction::RemoteUpsert => self.observable.has_quote,
            QuoteApprovalWorkerAction::OwnerApprove | QuoteApprovalWorkerAction::OwnerReject => {
                self.observable.quote_state == QuoteState::Pending
                    && self.quote_of_uri_matches_target
            }
            QuoteApprovalWorkerAction::Revoke => {
                self.requester_is_quote_author && self.observable.has_quote
            }
        }
    }

    pub(crate) fn worker_effect(self, action: QuoteApprovalWorkerAction) -> Self {
        if !self.worker_allows(action) {
            return self;
        }

        Self {
            observable: self.observable.domain_step(action),
            ..self
        }
    }
}

fn model_domain_step(
    state: QuoteApprovalModelState,
    action: QuoteApprovalAction,
) -> Option<QuoteApprovalModelState> {
    let observable = QuoteApprovalObservable {
        quote_state: state.quote_state,
        has_quote: state.has_quote,
        target_exists_locally: state.target_exists_locally,
        blocked_by_owner: state.blocked_by_owner,
        policy_allows: state.policy_allows,
    };
    let worker_action = match action {
        QuoteApprovalAction::RemoteUpsert => QuoteApprovalWorkerAction::RemoteUpsert,
        QuoteApprovalAction::OwnerApprove => QuoteApprovalWorkerAction::OwnerApprove,
        QuoteApprovalAction::OwnerReject => QuoteApprovalWorkerAction::OwnerReject,
        QuoteApprovalAction::Revoke => QuoteApprovalWorkerAction::Revoke,
    };

    Some(observable.domain_step(worker_action).to_model_state())
}

fn worker_initial_states() -> Vec<QuoteApprovalObservable> {
    let mut states = Vec::new();
    for quote_state in [
        QuoteState::Accepted,
        QuoteState::Pending,
        QuoteState::Rejected,
        QuoteState::Revoked,
    ] {
        for has_quote in [false, true] {
            for target_exists_locally in [false, true] {
                for blocked_by_owner in [false, true] {
                    for policy_allows in [false, true] {
                        states.push(QuoteApprovalObservable {
                            quote_state,
                            has_quote,
                            target_exists_locally,
                            blocked_by_owner,
                            policy_allows,
                        });
                    }
                }
            }
        }
    }
    states
}

fn worker_contexts(observable: QuoteApprovalObservable) -> Vec<QuoteApprovalWorkerContext> {
    let mut contexts = Vec::new();
    for quote_of_uri_matches_target in [false, true] {
        for requester_is_quote_author in [false, true] {
            contexts.push(QuoteApprovalWorkerContext {
                observable,
                quote_of_uri_matches_target,
                requester_is_quote_author,
            });
        }
    }
    contexts
}

pub(crate) fn check_quote_approval_refinement() {
    assert_model_matches_domain(&QuoteApprovalModel, model_domain_step);

    for observable in worker_initial_states() {
        for context in worker_contexts(observable) {
            assert_worker_refinement(
                vec![context.observable],
                |_| {
                    [
                        QuoteApprovalWorkerAction::RemoteUpsert,
                        QuoteApprovalWorkerAction::OwnerApprove,
                        QuoteApprovalWorkerAction::OwnerReject,
                        QuoteApprovalWorkerAction::Revoke,
                    ]
                    .to_vec()
                },
                |state, action| {
                    QuoteApprovalWorkerContext {
                        observable: state,
                        quote_of_uri_matches_target: context.quote_of_uri_matches_target,
                        requester_is_quote_author: context.requester_is_quote_author,
                    }
                    .worker_allows(action)
                },
                |state, action| {
                    QuoteApprovalWorkerContext {
                        observable: state,
                        quote_of_uri_matches_target: context.quote_of_uri_matches_target,
                        requester_is_quote_author: context.requester_is_quote_author,
                    }
                    .worker_effect(action)
                    .observable
                },
                |state, action| state.domain_step(action),
            );
        }
    }

    assert_eq!(
        QuoteState::Pending.quote_state_after_owner_action(OwnerQuoteAction::Approve),
        QuoteState::Accepted,
        "worker approve path must call quote_state_after_owner_action"
    );
}

#[cfg(test)]
mod tests {
    use super::check_quote_approval_refinement;

    #[test]
    fn quote_approval_refinement_holds() {
        check_quote_approval_refinement();
    }
}
