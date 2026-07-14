use cfwdon_domain::{
    DELIVERY_MAX_ATTEMPTS, DeliveryAttemptOutcome, OutboxDeliveryRecordState,
    generic_outbox_has_follower_targets, generic_outbox_parent_state_after_expand,
    is_delivery_terminal, next_delivery_attempt_count,
};

use crate::outbox_pipeline::{
    OutboxPipelineAction, OutboxPipelineModel, OutboxPipelineModelState, TargetSlotState,
    apply_outbox_pipeline_target_outcome, expand_outbox_pipeline_generic,
};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use stateright::Model;

fn model_domain_step(
    state: OutboxPipelineModelState,
    action: OutboxPipelineAction,
) -> Option<OutboxPipelineModelState> {
    OutboxPipelineModel.next_state(&state, action)
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

fn target_index(action: OutboxPipelineAction) -> Option<usize> {
    match action {
        OutboxPipelineAction::Target0Succeeds | OutboxPipelineAction::Target0Fails => Some(0),
        OutboxPipelineAction::Target1Succeeds | OutboxPipelineAction::Target1Fails => Some(1),
        OutboxPipelineAction::ExpandGeneric => None,
    }
}

/// Mirrors `partition_generic_outbox_deliveries_by_targets` plus expand/completion paths.
fn worker_allows(state: OutboxPipelineModelState, action: OutboxPipelineAction) -> bool {
    match action {
        OutboxPipelineAction::ExpandGeneric => state.generic_parent_queued(),
        _ => {
            let Some(index) = target_index(action) else {
                return false;
            };
            state.target_queued(index)
        }
    }
}

fn worker_effect(
    mut state: OutboxPipelineModelState,
    action: OutboxPipelineAction,
) -> OutboxPipelineModelState {
    match action {
        OutboxPipelineAction::ExpandGeneric => {
            if state.generic_parent_queued() {
                expand_outbox_pipeline_generic(&mut state);
            }
        }
        _ => {
            if let (Some(index), Some(outcome)) = (target_index(action), target_outcome(action)) {
                apply_outbox_pipeline_target_outcome(&mut state.targets[index], outcome);
            }
        }
    }
    state
}

fn domain_step(
    state: OutboxPipelineModelState,
    action: OutboxPipelineAction,
) -> OutboxPipelineModelState {
    model_domain_step(state, action).unwrap_or(state)
}

pub(crate) fn check_outbox_pipeline_refinement() {
    assert_model_matches_domain(&OutboxPipelineModel, model_domain_step);

    assert_worker_refinement(
        OutboxPipelineModel.init_states(),
        |state| {
            let mut actions = Vec::new();
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
            actions
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    for follower_target_count in 0..=2_u32 {
        assert_eq!(
            generic_outbox_has_follower_targets(follower_target_count as usize),
            follower_target_count > 0,
            "worker follower-target guard for count={follower_target_count}"
        );
        assert_eq!(
            generic_outbox_parent_state_after_expand(follower_target_count),
            if follower_target_count == 0 {
                OutboxDeliveryRecordState::Delivered
            } else {
                OutboxDeliveryRecordState::Expanded
            },
            "expand parent state for count={follower_target_count}"
        );
    }

    let mut slot = TargetSlotState {
        active: true,
        state: OutboxDeliveryRecordState::Queued,
        attempt_count: 0,
    };
    for attempt_count in 0..DELIVERY_MAX_ATTEMPTS {
        slot.attempt_count = attempt_count;
        slot.state = OutboxDeliveryRecordState::Queued;
        apply_outbox_pipeline_target_outcome(&mut slot, DeliveryAttemptOutcome::Failure);
        let terminal = is_delivery_terminal(next_delivery_attempt_count(attempt_count as i32));
        assert_eq!(
            slot.state == OutboxDeliveryRecordState::Failed,
            terminal,
            "target terminal state for attempt_count={attempt_count}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::check_outbox_pipeline_refinement;

    #[test]
    fn outbox_pipeline_refinement_holds() {
        check_outbox_pipeline_refinement();
    }
}
