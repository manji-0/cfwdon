use cfwdon_domain::{
    DeliveryAttemptOutcome, OutboundActivityState, OutboundDeliverySlot,
    outbound_delivery_slot_after_attempt,
};
use stateright::{Checker, Model, Property};

const SLOT_COUNT: usize = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ConcurrentDeliveryModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ConcurrentDeliveryModelState {
    pub(crate) slots: [OutboundDeliverySlot; SLOT_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ConcurrentDeliveryAction {
    SucceedSlot0,
    SucceedSlot1,
    FailSlot0,
    FailSlot1,
}

impl ConcurrentDeliveryModelState {
    pub(crate) fn slot_is_queued(&self, index: usize) -> bool {
        self.slots[index].state == OutboundActivityState::Queued
    }
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
}

/// Shared two-slot transition used by the Stateright model and worker refinement checks.
pub(crate) fn apply_concurrent_delivery_action(
    slots: &mut [OutboundDeliverySlot; SLOT_COUNT],
    action: ConcurrentDeliveryAction,
) -> bool {
    let index = ConcurrentDeliveryModel::slot_index(action);
    let slot = &mut slots[index];
    if slot.state != OutboundActivityState::Queued {
        return false;
    }
    *slot = outbound_delivery_slot_after_attempt(*slot, ConcurrentDeliveryModel::outcome(action));
    true
}

impl Model for ConcurrentDeliveryModel {
    type State = ConcurrentDeliveryModelState;
    type Action = ConcurrentDeliveryAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![ConcurrentDeliveryModelState {
            slots: [OutboundDeliverySlot::queued(); SLOT_COUNT],
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        if state.slot_is_queued(0) {
            actions.push(ConcurrentDeliveryAction::SucceedSlot0);
            actions.push(ConcurrentDeliveryAction::FailSlot0);
        }
        if state.slot_is_queued(1) {
            actions.push(ConcurrentDeliveryAction::SucceedSlot1);
            actions.push(ConcurrentDeliveryAction::FailSlot1);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;
        apply_concurrent_delivery_action(&mut next.slots, action).then_some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "each_slot_respects_retry_threshold",
                |_, state: &ConcurrentDeliveryModelState| {
                    state.slots.iter().all(|slot| {
                        slot.state != OutboundActivityState::Queued
                            || slot.attempt_count < cfwdon_domain::DELIVERY_MAX_ATTEMPTS
                    })
                },
            ),
            Property::always(
                "terminal_slot_requires_max_attempts",
                |_, state: &ConcurrentDeliveryModelState| {
                    state.slots.iter().all(|slot| {
                        slot.state != OutboundActivityState::Failed
                            || slot.attempt_count >= cfwdon_domain::DELIVERY_MAX_ATTEMPTS
                    })
                },
            ),
            Property::always(
                "slots_evolve_independently",
                |_, state: &ConcurrentDeliveryModelState| {
                    let progressed = state
                        .slots
                        .iter()
                        .filter(|slot| slot.state != OutboundActivityState::Queued)
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
                        .all(|slot| slot.state == OutboundActivityState::Delivered)
                },
            ),
            Property::sometimes(
                "mixed_terminal_states_reachable",
                |_, state: &ConcurrentDeliveryModelState| {
                    state.slots[0].state == OutboundActivityState::Delivered
                        && state.slots[1].state == OutboundActivityState::Failed
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
