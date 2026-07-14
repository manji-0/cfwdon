use cfwdon_domain::{
    DELIVERY_MAX_ATTEMPTS, DeliveryAttemptOutcome, FollowInboxResponse, OutboundActivityState,
    RemoteFollowState, follow_state_after_inbox_response, initial_remote_follow_state,
    is_delivery_terminal, next_delivery_attempt_count, outbound_terminal_failure_follow_state,
};

use crate::outbound_follow::{
    OutboundFollowAction, OutboundFollowModel, OutboundFollowModelState,
    apply_outbound_follow_delivery_outcome, apply_outbound_follow_inbox_response,
};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use stateright::Model;

fn model_domain_step(
    state: OutboundFollowModelState,
    action: OutboundFollowAction,
) -> Option<OutboundFollowModelState> {
    OutboundFollowModel.next_state(&state, action)
}

/// Mirrors `reconcile_outbound_activity_terminal_failure` follow-state guard.
fn worker_reconciles_follow_on_terminal_failure(activity_type: &str) -> bool {
    outbound_terminal_failure_follow_state(activity_type).is_some()
}

fn worker_allows(state: OutboundFollowModelState, action: OutboundFollowAction) -> bool {
    match action {
        OutboundFollowAction::DeliverySucceeds | OutboundFollowAction::DeliveryFails => {
            state.outbound_queued()
        }
        OutboundFollowAction::ReceiveFollowAccept | OutboundFollowAction::ReceiveFollowReject => {
            state.activity_is_follow
        }
    }
}

fn worker_effect(
    mut state: OutboundFollowModelState,
    action: OutboundFollowAction,
) -> OutboundFollowModelState {
    match action {
        OutboundFollowAction::DeliverySucceeds => {
            apply_outbound_follow_delivery_outcome(&mut state, DeliveryAttemptOutcome::Success);
        }
        OutboundFollowAction::DeliveryFails => {
            apply_outbound_follow_delivery_outcome(&mut state, DeliveryAttemptOutcome::Failure);
        }
        OutboundFollowAction::ReceiveFollowAccept => {
            apply_outbound_follow_inbox_response(&mut state, FollowInboxResponse::Accept);
        }
        OutboundFollowAction::ReceiveFollowReject => {
            apply_outbound_follow_inbox_response(&mut state, FollowInboxResponse::Reject);
        }
    }
    state
}

fn domain_step(
    state: OutboundFollowModelState,
    action: OutboundFollowAction,
) -> OutboundFollowModelState {
    model_domain_step(state, action).unwrap_or(state)
}

pub(crate) fn check_outbound_follow_refinement() {
    assert_model_matches_domain(&OutboundFollowModel, model_domain_step);

    assert_worker_refinement(
        OutboundFollowModel.init_states(),
        |state| {
            let mut actions = Vec::new();
            if state.outbound_queued() {
                actions.push(OutboundFollowAction::DeliverySucceeds);
                actions.push(OutboundFollowAction::DeliveryFails);
            }
            if state.activity_is_follow {
                actions.push(OutboundFollowAction::ReceiveFollowAccept);
                actions.push(OutboundFollowAction::ReceiveFollowReject);
            }
            actions
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    assert_eq!(
        initial_remote_follow_state(true),
        RemoteFollowState::Pending
    );
    assert_eq!(
        initial_remote_follow_state(false),
        RemoteFollowState::Accepted
    );

    assert!(worker_reconciles_follow_on_terminal_failure("Follow"));
    assert!(!worker_reconciles_follow_on_terminal_failure("Undo"));

    assert_eq!(
        follow_state_after_inbox_response(RemoteFollowState::Pending, FollowInboxResponse::Accept),
        RemoteFollowState::Accepted
    );
    assert_eq!(
        follow_state_after_inbox_response(RemoteFollowState::Pending, FollowInboxResponse::Reject),
        RemoteFollowState::Rejected
    );

    for attempt_count in 0..DELIVERY_MAX_ATTEMPTS {
        let terminal = is_delivery_terminal(next_delivery_attempt_count(attempt_count as i32));
        let mut state = OutboundFollowModelState {
            outbound_state: OutboundActivityState::Queued,
            follow_state: RemoteFollowState::Pending,
            attempt_count,
            activity_is_follow: true,
        };
        apply_outbound_follow_delivery_outcome(&mut state, DeliveryAttemptOutcome::Failure);
        if terminal {
            assert_eq!(state.outbound_state, OutboundActivityState::Failed);
            assert_eq!(state.follow_state, RemoteFollowState::Failed);
        } else {
            assert_eq!(state.outbound_state, OutboundActivityState::Queued);
            assert_eq!(state.follow_state, RemoteFollowState::Pending);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_outbound_follow_refinement;

    #[test]
    fn outbound_follow_refinement_holds() {
        check_outbound_follow_refinement();
    }
}
