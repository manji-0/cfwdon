use cfwdon_domain::{
    DELIVERY_MAX_ATTEMPTS, DeliveryAttemptOutcome, OUTBOX_DELIVERY_CONCURRENCY,
    OutboundActivityState, OutboundDeliverySlot, is_delivery_terminal, next_delivery_attempt_count,
    outbound_delivery_slot_after_attempt,
};

use crate::outbox_delivery_pool::{
    OutboxDeliveryPoolAction, OutboxDeliveryPoolModel, OutboxDeliveryPoolModelState,
    apply_outbox_delivery_pool_outcome,
};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use stateright::Model;

fn model_domain_step(
    state: OutboxDeliveryPoolModelState,
    action: OutboxDeliveryPoolAction,
) -> Option<OutboxDeliveryPoolModelState> {
    OutboxDeliveryPoolModel.next_state(&state, action)
}

/// Mirrors one `buffer_unordered` completion updating a queued delivery row.
fn worker_slot_after_attempt(
    attempt_count: u32,
    outcome: DeliveryAttemptOutcome,
) -> OutboundDeliverySlot {
    outbound_delivery_slot_after_attempt(
        OutboundDeliverySlot {
            state: OutboundActivityState::Queued,
            attempt_count,
        },
        outcome,
    )
}

/// Mirrors `delivery.rs` terminal failure guard after a failed send attempt.
fn worker_marks_terminal_failure(attempt_count: u32) -> bool {
    is_delivery_terminal(next_delivery_attempt_count(attempt_count as i32))
}

fn worker_allows(state: OutboxDeliveryPoolModelState, action: OutboxDeliveryPoolAction) -> bool {
    let attempt_index = match action {
        OutboxDeliveryPoolAction::SucceedAt(index) | OutboxDeliveryPoolAction::FailAt(index) => {
            index
        }
    };
    state.has_queued_slots_at(attempt_index)
}

fn worker_effect(
    mut state: OutboxDeliveryPoolModelState,
    action: OutboxDeliveryPoolAction,
) -> OutboxDeliveryPoolModelState {
    match action {
        OutboxDeliveryPoolAction::SucceedAt(attempt_index) => {
            apply_outbox_delivery_pool_outcome(
                &mut state,
                attempt_index,
                DeliveryAttemptOutcome::Success,
            );
        }
        OutboxDeliveryPoolAction::FailAt(attempt_index) => {
            apply_outbox_delivery_pool_outcome(
                &mut state,
                attempt_index,
                DeliveryAttemptOutcome::Failure,
            );
        }
    }
    state
}

fn domain_step(
    state: OutboxDeliveryPoolModelState,
    action: OutboxDeliveryPoolAction,
) -> OutboxDeliveryPoolModelState {
    model_domain_step(state, action).unwrap_or(state)
}

pub(crate) fn check_outbox_delivery_pool_refinement() {
    assert_model_matches_domain(&OutboxDeliveryPoolModel, model_domain_step);

    assert_worker_refinement(
        OutboxDeliveryPoolModel.init_states(),
        |state| {
            let mut actions = Vec::new();
            for attempt_index in 0..DELIVERY_MAX_ATTEMPTS as u8 {
                if state.has_queued_slots_at(attempt_index) {
                    actions.push(OutboxDeliveryPoolAction::SucceedAt(attempt_index));
                    actions.push(OutboxDeliveryPoolAction::FailAt(attempt_index));
                }
            }
            actions
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    for attempt_count in 0..DELIVERY_MAX_ATTEMPTS {
        let failure_slot =
            worker_slot_after_attempt(attempt_count, DeliveryAttemptOutcome::Failure);
        assert_eq!(
            worker_marks_terminal_failure(attempt_count),
            failure_slot.state == OutboundActivityState::Failed,
            "terminal guard must match slot transition for attempt_count={attempt_count}"
        );
        assert_eq!(
            worker_marks_terminal_failure(attempt_count),
            is_delivery_terminal(next_delivery_attempt_count(attempt_count as i32)),
            "worker terminal guard must use domain is_delivery_terminal"
        );
    }

    let success_slot = worker_slot_after_attempt(0, DeliveryAttemptOutcome::Success);
    assert_eq!(success_slot.state, OutboundActivityState::Delivered);

    assert_eq!(OUTBOX_DELIVERY_CONCURRENCY, 8);
}

#[cfg(test)]
mod tests {
    use super::check_outbox_delivery_pool_refinement;

    #[test]
    fn outbox_delivery_pool_refinement_holds() {
        check_outbox_delivery_pool_refinement();
    }
}
