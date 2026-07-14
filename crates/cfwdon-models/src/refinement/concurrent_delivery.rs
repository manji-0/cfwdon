use cfwdon_domain::{
    DELIVERY_MAX_ATTEMPTS, DeliveryAttemptOutcome, OutboundActivityState, OutboundDeliverySlot,
    is_delivery_terminal, next_delivery_attempt_count, outbound_delivery_slot_after_attempt,
};

use crate::concurrent_delivery::{
    ConcurrentDeliveryAction, ConcurrentDeliveryModel, ConcurrentDeliveryModelState,
    apply_concurrent_delivery_action,
};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use stateright::Model;

fn model_domain_step(
    state: ConcurrentDeliveryModelState,
    action: ConcurrentDeliveryAction,
) -> Option<ConcurrentDeliveryModelState> {
    ConcurrentDeliveryModel.next_state(&state, action)
}

fn slot_index(action: ConcurrentDeliveryAction) -> usize {
    match action {
        ConcurrentDeliveryAction::SucceedSlot0 | ConcurrentDeliveryAction::FailSlot0 => 0,
        ConcurrentDeliveryAction::SucceedSlot1 | ConcurrentDeliveryAction::FailSlot1 => 1,
    }
}

/// Mirrors one concurrent `buffer_unordered` completion on a single delivery row.
fn worker_slot_after_attempt(
    slot: OutboundDeliverySlot,
    outcome: DeliveryAttemptOutcome,
) -> OutboundDeliverySlot {
    outbound_delivery_slot_after_attempt(slot, outcome)
}

fn worker_allows(state: ConcurrentDeliveryModelState, action: ConcurrentDeliveryAction) -> bool {
    state.slot_is_queued(slot_index(action))
}

fn worker_effect(
    mut state: ConcurrentDeliveryModelState,
    action: ConcurrentDeliveryAction,
) -> ConcurrentDeliveryModelState {
    apply_concurrent_delivery_action(&mut state.slots, action);
    state
}

fn domain_step(
    state: ConcurrentDeliveryModelState,
    action: ConcurrentDeliveryAction,
) -> ConcurrentDeliveryModelState {
    model_domain_step(state, action).unwrap_or(state)
}

pub(crate) fn check_concurrent_delivery_refinement() {
    assert_model_matches_domain(&ConcurrentDeliveryModel, model_domain_step);

    assert_worker_refinement(
        ConcurrentDeliveryModel.init_states(),
        |state| {
            let mut actions = Vec::new();
            if state.slot_is_queued(0) {
                actions.push(ConcurrentDeliveryAction::SucceedSlot0);
                actions.push(ConcurrentDeliveryAction::FailSlot0);
            }
            if state.slot_is_queued(1) {
                actions.push(ConcurrentDeliveryAction::SucceedSlot1);
                actions.push(ConcurrentDeliveryAction::FailSlot1);
            }
            actions
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    let queued = OutboundDeliverySlot::queued();
    for attempt_count in 0..DELIVERY_MAX_ATTEMPTS {
        let slot = OutboundDeliverySlot {
            attempt_count,
            ..queued
        };
        let failure_slot = worker_slot_after_attempt(slot, DeliveryAttemptOutcome::Failure);
        let terminal = is_delivery_terminal(next_delivery_attempt_count(attempt_count as i32));
        assert_eq!(
            failure_slot.state == OutboundActivityState::Failed,
            terminal,
            "slot transition must match worker terminal guard for attempt_count={attempt_count}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::check_concurrent_delivery_refinement;

    #[test]
    fn concurrent_delivery_refinement_holds() {
        check_concurrent_delivery_refinement();
    }
}
