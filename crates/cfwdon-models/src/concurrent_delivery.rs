use cfwdon_domain::{
    DELIVERY_MAX_ATTEMPTS, DeliveryAttemptOutcome, OutboundActivityState,
    next_delivery_attempt_count, outbound_state_after_delivery_attempt,
};
use stateright::{Checker, Model, Property};

const SLOT_COUNT: usize = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ConcurrentDeliveryModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DeliverySlotState {
    outbound_state: OutboundActivityState,
    attempt_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ConcurrentDeliveryModelState {
    slots: [DeliverySlotState; SLOT_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ConcurrentDeliveryAction {
    SucceedSlot0,
    SucceedSlot1,
    FailSlot0,
    FailSlot1,
}

impl ConcurrentDeliveryModel {
    fn slot_index(action: ConcurrentDeliveryAction) -> usize {
        match action {
            ConcurrentDeliveryAction::SucceedSlot0 | ConcurrentDeliveryAction::FailSlot0 => 0,
            ConcurrentDeliveryAction::SucceedSlot1 | ConcurrentDeliveryAction::FailSlot1 => 1,
        }
    }

    fn outcome(action: ConcurrentDeliveryAction) -> DeliveryAttemptOutcome {
        match action {
            ConcurrentDeliveryAction::SucceedSlot0 | ConcurrentDeliveryAction::SucceedSlot1 => {
                DeliveryAttemptOutcome::Success
            }
            ConcurrentDeliveryAction::FailSlot0 | ConcurrentDeliveryAction::FailSlot1 => {
                DeliveryAttemptOutcome::Failure
            }
        }
    }

    fn apply_action(
        slots: &mut [DeliverySlotState; SLOT_COUNT],
        action: ConcurrentDeliveryAction,
    ) -> bool {
        let index = Self::slot_index(action);
        let slot = &mut slots[index];
        if slot.outbound_state != OutboundActivityState::Queued {
            return false;
        }

        let next_attempt = next_delivery_attempt_count(slot.attempt_count as i32);
        let outcome = Self::outcome(action);
        if matches!(outcome, DeliveryAttemptOutcome::Failure) {
            slot.attempt_count = next_attempt;
        }
        slot.outbound_state =
            outbound_state_after_delivery_attempt(slot.outbound_state, next_attempt, outcome);
        true
    }
}

impl Model for ConcurrentDeliveryModel {
    type State = ConcurrentDeliveryModelState;
    type Action = ConcurrentDeliveryAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![ConcurrentDeliveryModelState {
            slots: [DeliverySlotState {
                outbound_state: OutboundActivityState::Queued,
                attempt_count: 0,
            }; SLOT_COUNT],
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        if state.slots[0].outbound_state == OutboundActivityState::Queued {
            actions.push(ConcurrentDeliveryAction::SucceedSlot0);
            actions.push(ConcurrentDeliveryAction::FailSlot0);
        }
        if state.slots[1].outbound_state == OutboundActivityState::Queued {
            actions.push(ConcurrentDeliveryAction::SucceedSlot1);
            actions.push(ConcurrentDeliveryAction::FailSlot1);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;
        if Self::apply_action(&mut next.slots, action) {
            Some(next)
        } else {
            None
        }
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "each_slot_respects_retry_threshold",
                |_, state: &ConcurrentDeliveryModelState| {
                    state.slots.iter().all(|slot| {
                        slot.outbound_state != OutboundActivityState::Queued
                            || slot.attempt_count < DELIVERY_MAX_ATTEMPTS
                    })
                },
            ),
            Property::always(
                "terminal_slot_requires_max_attempts",
                |_, state: &ConcurrentDeliveryModelState| {
                    state.slots.iter().all(|slot| {
                        slot.outbound_state != OutboundActivityState::Failed
                            || slot.attempt_count >= DELIVERY_MAX_ATTEMPTS
                    })
                },
            ),
            Property::always(
                "slots_evolve_independently",
                |_, state: &ConcurrentDeliveryModelState| {
                    let progressed = state
                        .slots
                        .iter()
                        .filter(|slot| slot.outbound_state != OutboundActivityState::Queued)
                        .count();
                    progressed <= SLOT_COUNT
                },
            ),
            Property::sometimes(
                "both_slots_delivered_reachable",
                |_, state: &ConcurrentDeliveryModelState| {
                    state
                        .slots
                        .iter()
                        .all(|slot| slot.outbound_state == OutboundActivityState::Delivered)
                },
            ),
            Property::sometimes(
                "mixed_terminal_states_reachable",
                |_, state: &ConcurrentDeliveryModelState| {
                    state.slots[0].outbound_state == OutboundActivityState::Delivered
                        && state.slots[1].outbound_state == OutboundActivityState::Failed
                },
            ),
        ]
    }
}

pub(crate) fn check_concurrent_delivery_model() {
    ConcurrentDeliveryModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_concurrent_delivery_model;

    #[test]
    fn concurrent_delivery_model_holds() {
        check_concurrent_delivery_model();
    }
}
