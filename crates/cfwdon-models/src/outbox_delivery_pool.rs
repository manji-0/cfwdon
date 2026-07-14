use cfwdon_domain::{
    DELIVERY_MAX_ATTEMPTS, DeliveryAttemptOutcome, OUTBOX_DELIVERY_CONCURRENCY,
    OutboundActivityState, OutboundDeliverySlot, outbound_delivery_slot_after_attempt,
    outbox_delivery_pool_size,
};
use stateright::{Checker, Model, Property};

const POOL_SIZE: u8 = OUTBOX_DELIVERY_CONCURRENCY as u8;
const MAX_QUEUED_ATTEMPT_INDEX: usize = (DELIVERY_MAX_ATTEMPTS - 1) as usize;

#[derive(Clone, Copy, Debug)]
pub(crate) struct OutboxDeliveryPoolModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct OutboxDeliveryPoolModelState {
    queued_at: [u8; DELIVERY_MAX_ATTEMPTS as usize],
    delivered: u8,
    failed: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OutboxDeliveryPoolAction {
    SucceedAt(u8),
    FailAt(u8),
}

impl OutboxDeliveryPoolModel {
    fn total_slots(state: &OutboxDeliveryPoolModelState) -> u8 {
        state.delivered + state.failed + state.queued_at.iter().sum::<u8>()
    }

    fn apply_outcome(
        state: &mut OutboxDeliveryPoolModelState,
        attempt_index: u8,
        outcome: DeliveryAttemptOutcome,
    ) -> bool {
        let index = attempt_index as usize;
        if index >= state.queued_at.len() || state.queued_at[index] == 0 {
            return false;
        }

        let slot = OutboundDeliverySlot {
            state: OutboundActivityState::Queued,
            attempt_count: attempt_index as u32,
        };
        let next = outbound_delivery_slot_after_attempt(slot, outcome);
        state.queued_at[index] -= 1;
        match next.state {
            OutboundActivityState::Queued => {
                let next_index = next.attempt_count as usize;
                if next_index < state.queued_at.len() {
                    state.queued_at[next_index] += 1;
                }
            }
            OutboundActivityState::Delivered => state.delivered += 1,
            OutboundActivityState::Failed => state.failed += 1,
        }
        true
    }
}

impl Model for OutboxDeliveryPoolModel {
    type State = OutboxDeliveryPoolModelState;
    type Action = OutboxDeliveryPoolAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut state = OutboxDeliveryPoolModelState {
            queued_at: [0; DELIVERY_MAX_ATTEMPTS as usize],
            delivered: 0,
            failed: 0,
        };
        state.queued_at[0] = outbox_delivery_pool_size(POOL_SIZE, OUTBOX_DELIVERY_CONCURRENCY);
        vec![state]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        for attempt_index in 0..=MAX_QUEUED_ATTEMPT_INDEX as u8 {
            if state.queued_at[attempt_index as usize] > 0 {
                actions.push(OutboxDeliveryPoolAction::SucceedAt(attempt_index));
                actions.push(OutboxDeliveryPoolAction::FailAt(attempt_index));
            }
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;
        let applied = match action {
            OutboxDeliveryPoolAction::SucceedAt(attempt_index) => {
                Self::apply_outcome(&mut next, attempt_index, DeliveryAttemptOutcome::Success)
            }
            OutboxDeliveryPoolAction::FailAt(attempt_index) => {
                Self::apply_outcome(&mut next, attempt_index, DeliveryAttemptOutcome::Failure)
            }
        };
        applied.then_some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "pool_size_matches_outbox_concurrency",
                |_, state: &OutboxDeliveryPoolModelState| {
                    Self::total_slots(state)
                        == outbox_delivery_pool_size(POOL_SIZE, OUTBOX_DELIVERY_CONCURRENCY)
                },
            ),
            Property::always(
                "queued_slots_respect_retry_threshold",
                |_, state: &OutboxDeliveryPoolModelState| {
                    state
                        .queued_at
                        .iter()
                        .enumerate()
                        .all(|(index, count)| *count == 0 || (index as u32) < DELIVERY_MAX_ATTEMPTS)
                },
            ),
            Property::always(
                "failed_slots_imply_terminal_retry_count",
                |_, state: &OutboxDeliveryPoolModelState| {
                    state.failed == 0
                        || state.queued_at[(DELIVERY_MAX_ATTEMPTS - 1) as usize] < POOL_SIZE
                },
            ),
            Property::always(
                "delivered_and_failed_never_return_to_queue",
                |_, state: &OutboxDeliveryPoolModelState| {
                    state.delivered <= POOL_SIZE && state.failed <= POOL_SIZE
                },
            ),
            Property::sometimes(
                "all_delivered_reachable",
                |_, state: &OutboxDeliveryPoolModelState| {
                    state.delivered == POOL_SIZE
                        && state.failed == 0
                        && state.queued_at.iter().all(|count| *count == 0)
                },
            ),
            Property::sometimes(
                "mixed_terminal_pool_reachable",
                |_, state: &OutboxDeliveryPoolModelState| state.delivered > 0 && state.failed > 0,
            ),
        ]
    }
}

pub(crate) fn check_outbox_delivery_pool_model() {
    OutboxDeliveryPoolModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_outbox_delivery_pool_model;

    #[test]
    fn outbox_delivery_pool_model_holds() {
        check_outbox_delivery_pool_model();
    }
}
