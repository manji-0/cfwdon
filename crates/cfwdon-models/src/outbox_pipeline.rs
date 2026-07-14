use cfwdon_domain::{
    DELIVERY_MAX_ATTEMPTS, DeliveryAttemptOutcome, OutboxDeliveryRecordState,
    generic_outbox_has_follower_targets, generic_outbox_parent_state_after_expand,
    next_delivery_attempt_count, outbox_delivery_state_after_attempt, outbox_expand_slot_count,
};
use stateright::{Checker, Model, Property};

const MAX_TARGET_SLOTS: usize = 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct OutboxPipelineModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TargetSlotState {
    pub(crate) active: bool,
    pub(crate) state: OutboxDeliveryRecordState,
    pub(crate) attempt_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct OutboxPipelineModelState {
    pub(crate) generic_parent: OutboxDeliveryRecordState,
    pub(crate) follower_target_count: u32,
    pub(crate) targets: [TargetSlotState; MAX_TARGET_SLOTS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OutboxPipelineAction {
    ExpandGeneric,
    Target0Succeeds,
    Target0Fails,
    Target1Succeeds,
    Target1Fails,
}

impl OutboxPipelineModelState {
    pub(crate) fn generic_parent_queued(&self) -> bool {
        self.generic_parent == OutboxDeliveryRecordState::Queued
    }

    pub(crate) fn target_queued(&self, index: usize) -> bool {
        self.targets[index].active && self.targets[index].state == OutboxDeliveryRecordState::Queued
    }

    /// Mirrors `partition_generic_outbox_deliveries_by_targets` follower guard.
    pub(crate) fn worker_has_follower_targets(&self) -> bool {
        generic_outbox_has_follower_targets(self.follower_target_count as usize)
    }
}

pub(crate) fn activate_outbox_pipeline_targets(state: &mut OutboxPipelineModelState) {
    let slot_count =
        outbox_expand_slot_count(state.follower_target_count, MAX_TARGET_SLOTS as u32) as usize;
    for slot in state.targets.iter_mut().take(slot_count) {
        slot.active = true;
        slot.state = OutboxDeliveryRecordState::Queued;
        slot.attempt_count = 0;
    }
}

pub(crate) fn expand_outbox_pipeline_generic(state: &mut OutboxPipelineModelState) -> bool {
    if state.generic_parent != OutboxDeliveryRecordState::Queued {
        return false;
    }
    state.generic_parent = generic_outbox_parent_state_after_expand(state.follower_target_count);
    if state.generic_parent == OutboxDeliveryRecordState::Expanded {
        activate_outbox_pipeline_targets(state);
    }
    true
}

pub(crate) fn apply_outbox_pipeline_target_outcome(
    slot: &mut TargetSlotState,
    outcome: DeliveryAttemptOutcome,
) -> bool {
    if !slot.active || slot.state != OutboxDeliveryRecordState::Queued {
        return false;
    }
    let next_attempt = next_delivery_attempt_count(slot.attempt_count as i32);
    if matches!(outcome, DeliveryAttemptOutcome::Failure) {
        slot.attempt_count = next_attempt;
    }
    slot.state = outbox_delivery_state_after_attempt(slot.state, next_attempt, outcome);
    true
}

impl OutboxPipelineModel {
    fn target_action_index(action: OutboxPipelineAction) -> Option<usize> {
        match action {
            OutboxPipelineAction::Target0Succeeds | OutboxPipelineAction::Target0Fails => Some(0),
            OutboxPipelineAction::Target1Succeeds | OutboxPipelineAction::Target1Fails => Some(1),
            OutboxPipelineAction::ExpandGeneric => None,
        }
    }

    fn target_outcome(action: OutboxPipelineAction) -> Option<DeliveryAttemptOutcome> {
        match action {
            OutboxPipelineAction::Target0Succeeds | OutboxPipelineAction::Target1Succeeds => {
                Some(DeliveryAttemptOutcome::Success)
            }
            OutboxPipelineAction::Target0Fails | OutboxPipelineAction::Target1Fails => {
                Some(DeliveryAttemptOutcome::Failure)
            }
            OutboxPipelineAction::ExpandGeneric => None,
        }
    }
}

impl Model for OutboxPipelineModel {
    type State = OutboxPipelineModelState;
    type Action = OutboxPipelineAction;

    fn init_states(&self) -> Vec<Self::State> {
        [0_u32, 1, 2]
            .into_iter()
            .map(|follower_target_count| OutboxPipelineModelState {
                generic_parent: OutboxDeliveryRecordState::Queued,
                follower_target_count,
                targets: [TargetSlotState {
                    active: false,
                    state: OutboxDeliveryRecordState::Queued,
                    attempt_count: 0,
                }; MAX_TARGET_SLOTS],
            })
            .collect()
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        if state.generic_parent_queued() {
            actions.push(OutboxPipelineAction::ExpandGeneric);
        }
        if state.target_queued(0) {
            actions.push(OutboxPipelineAction::Target0Succeeds);
            actions.push(OutboxPipelineAction::Target0Fails);
        }
        if state.target_queued(1) {
            actions.push(OutboxPipelineAction::Target1Succeeds);
            actions.push(OutboxPipelineAction::Target1Fails);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            OutboxPipelineAction::ExpandGeneric => {
                expand_outbox_pipeline_generic(&mut next).then_some(next)
            }
            _ => {
                let index = Self::target_action_index(action)?;
                let outcome = Self::target_outcome(action)?;
                apply_outbox_pipeline_target_outcome(&mut next.targets[index], outcome)
                    .then_some(next)
            }
        }
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "expanded_parent_requires_follower_targets",
                |_, state: &OutboxPipelineModelState| {
                    state.generic_parent != OutboxDeliveryRecordState::Expanded
                        || state.follower_target_count > 0
                },
            ),
            Property::always(
                "delivered_parent_without_targets_only_when_empty",
                |_, state: &OutboxPipelineModelState| {
                    state.generic_parent != OutboxDeliveryRecordState::Delivered
                        || state.follower_target_count == 0
                },
            ),
            Property::always(
                "expanded_parent_has_active_target_slots",
                |_, state: &OutboxPipelineModelState| {
                    state.generic_parent != OutboxDeliveryRecordState::Expanded
                        || state.targets.iter().any(|slot| slot.active)
                },
            ),
            Property::always(
                "inactive_target_slots_never_attempt_delivery",
                |_, state: &OutboxPipelineModelState| {
                    state
                        .targets
                        .iter()
                        .all(|slot| slot.active || slot.attempt_count == 0)
                },
            ),
            Property::always(
                "active_targets_respect_retry_threshold",
                |_, state: &OutboxPipelineModelState| {
                    state.targets.iter().all(|slot| {
                        !slot.active
                            || slot.state != OutboxDeliveryRecordState::Queued
                            || slot.attempt_count < DELIVERY_MAX_ATTEMPTS
                    })
                },
            ),
            Property::always(
                "failed_target_requires_terminal_attempts",
                |_, state: &OutboxPipelineModelState| {
                    state.targets.iter().all(|slot| {
                        !slot.active
                            || slot.state != OutboxDeliveryRecordState::Failed
                            || slot.attempt_count >= DELIVERY_MAX_ATTEMPTS
                    })
                },
            ),
            Property::sometimes(
                "completed_without_targets_reachable",
                |_, state: &OutboxPipelineModelState| {
                    state.generic_parent == OutboxDeliveryRecordState::Delivered
                        && state.follower_target_count == 0
                },
            ),
            Property::sometimes(
                "all_active_targets_delivered_reachable",
                |_, state: &OutboxPipelineModelState| {
                    state.targets.iter().any(|slot| slot.active)
                        && state
                            .targets
                            .iter()
                            .filter(|slot| slot.active)
                            .all(|slot| slot.state == OutboxDeliveryRecordState::Delivered)
                },
            ),
        ]
    }
}

pub(crate) fn check_outbox_pipeline_model() {
    OutboxPipelineModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_outbox_pipeline_model;

    #[test]
    fn outbox_pipeline_model_holds() {
        check_outbox_pipeline_model();
    }
}
