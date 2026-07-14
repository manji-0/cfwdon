use cfwdon_domain::{
    DELIVERY_MAX_ATTEMPTS, DeliveryAttemptOutcome, FollowInboxResponse, OutboundActivityState,
    RemoteFollowState, follow_state_after_inbox_response, initial_remote_follow_state,
    next_delivery_attempt_count, outbound_state_after_delivery_attempt,
    reconcile_pending_follow_on_outbound_terminal_failure,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct OutboundFollowModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct OutboundFollowModelState {
    pub(crate) outbound_state: OutboundActivityState,
    pub(crate) follow_state: RemoteFollowState,
    pub(crate) attempt_count: u32,
    pub(crate) activity_is_follow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OutboundFollowAction {
    DeliverySucceeds,
    DeliveryFails,
    ReceiveFollowAccept,
    ReceiveFollowReject,
}

impl OutboundFollowModelState {
    pub(crate) fn outbound_queued(&self) -> bool {
        self.outbound_state == OutboundActivityState::Queued
    }
}

pub(crate) fn apply_outbound_follow_delivery_outcome(
    state: &mut OutboundFollowModelState,
    outcome: DeliveryAttemptOutcome,
) -> bool {
    if state.outbound_state != OutboundActivityState::Queued {
        return false;
    }
    let next_attempt = next_delivery_attempt_count(state.attempt_count as i32);
    if matches!(outcome, DeliveryAttemptOutcome::Failure) {
        state.attempt_count = next_attempt;
    }
    state.outbound_state =
        outbound_state_after_delivery_attempt(state.outbound_state, next_attempt, outcome);
    if state.outbound_state == OutboundActivityState::Failed {
        let activity_type = if state.activity_is_follow {
            "Follow"
        } else {
            "Undo"
        };
        state.follow_state = reconcile_pending_follow_on_outbound_terminal_failure(
            state.follow_state,
            activity_type,
        );
    }
    true
}

pub(crate) fn apply_outbound_follow_inbox_response(
    state: &mut OutboundFollowModelState,
    response: FollowInboxResponse,
) -> bool {
    if !state.activity_is_follow {
        return false;
    }
    state.follow_state = follow_state_after_inbox_response(state.follow_state, response);
    true
}

impl Model for OutboundFollowModel {
    type State = OutboundFollowModelState;
    type Action = OutboundFollowAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();

        for actor_locked in [false, true] {
            for activity_is_follow in [false, true] {
                states.push(OutboundFollowModelState {
                    outbound_state: OutboundActivityState::Queued,
                    follow_state: initial_remote_follow_state(actor_locked),
                    attempt_count: 0,
                    activity_is_follow,
                });
            }
        }

        states
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        if state.outbound_queued() {
            actions.push(OutboundFollowAction::DeliverySucceeds);
            actions.push(OutboundFollowAction::DeliveryFails);
        }
        if state.activity_is_follow {
            actions.push(OutboundFollowAction::ReceiveFollowAccept);
            actions.push(OutboundFollowAction::ReceiveFollowReject);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        let applied = match action {
            OutboundFollowAction::DeliverySucceeds => {
                apply_outbound_follow_delivery_outcome(&mut next, DeliveryAttemptOutcome::Success)
            }
            OutboundFollowAction::DeliveryFails => {
                apply_outbound_follow_delivery_outcome(&mut next, DeliveryAttemptOutcome::Failure)
            }
            OutboundFollowAction::ReceiveFollowAccept => {
                apply_outbound_follow_inbox_response(&mut next, FollowInboxResponse::Accept)
            }
            OutboundFollowAction::ReceiveFollowReject => {
                apply_outbound_follow_inbox_response(&mut next, FollowInboxResponse::Reject)
            }
        };

        applied.then_some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "queued_attempts_below_terminal_threshold",
                |_, state: &OutboundFollowModelState| {
                    state.outbound_state != OutboundActivityState::Queued
                        || state.attempt_count < DELIVERY_MAX_ATTEMPTS
                },
            ),
            Property::always(
                "terminal_outbound_requires_max_attempts",
                |_, state: &OutboundFollowModelState| {
                    state.outbound_state != OutboundActivityState::Failed
                        || state.attempt_count >= DELIVERY_MAX_ATTEMPTS
                },
            ),
            Property::always(
                "failed_follow_requires_follow_activity",
                |_, state: &OutboundFollowModelState| {
                    state.follow_state != RemoteFollowState::Failed || state.activity_is_follow
                },
            ),
            Property::always(
                "non_follow_terminal_does_not_fail_pending_follow",
                |_, state: &OutboundFollowModelState| {
                    !(!state.activity_is_follow
                        && state.outbound_state == OutboundActivityState::Failed
                        && state.follow_state == RemoteFollowState::Failed)
                },
            ),
            Property::always(
                "failed_follow_implies_terminal_outbound",
                |_, state: &OutboundFollowModelState| {
                    state.follow_state != RemoteFollowState::Failed
                        || state.outbound_state == OutboundActivityState::Failed
                },
            ),
            Property::always(
                "rejected_follow_requires_follow_activity",
                |_, state: &OutboundFollowModelState| {
                    state.follow_state != RemoteFollowState::Rejected || state.activity_is_follow
                },
            ),
            Property::sometimes(
                "delivered_reachable",
                |_, state: &OutboundFollowModelState| {
                    state.outbound_state == OutboundActivityState::Delivered
                },
            ),
            Property::sometimes(
                "failed_outbound_reachable",
                |_, state: &OutboundFollowModelState| {
                    state.outbound_state == OutboundActivityState::Failed
                },
            ),
            Property::sometimes(
                "failed_follow_reachable",
                |_, state: &OutboundFollowModelState| {
                    state.follow_state == RemoteFollowState::Failed
                },
            ),
            Property::sometimes(
                "rejected_follow_reachable",
                |_, state: &OutboundFollowModelState| {
                    state.follow_state == RemoteFollowState::Rejected
                },
            ),
        ]
    }
}

pub(crate) fn check_outbound_follow_model() {
    OutboundFollowModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_outbound_follow_model;

    #[test]
    fn outbound_follow_model_holds() {
        check_outbound_follow_model();
    }
}
