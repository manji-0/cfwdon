use cfwdon_domain::{
    FollowRequestScenario, LocalFollowRequestState, LocalFollowState,
    RemoteInboundFollowRequestState, authorize_local_follow_request,
    initial_local_follow_request_state, reject_local_follow_request,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalFollowRequestModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LocalFollowRequestAction {
    Authorize,
    Reject,
}

/// Mirrors `authorize_pending_follow_request` / `reject_pending_follow_request` guards.
pub(crate) fn local_follow_request_action_allowed(state: &LocalFollowRequestState) -> bool {
    state.local_follow == Some(LocalFollowState::Pending)
        || state.remote_request == RemoteInboundFollowRequestState::Queued
}

impl Model for LocalFollowRequestModel {
    type State = LocalFollowRequestState;
    type Action = LocalFollowRequestAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();

        for target_locked in [false, true] {
            states.push(initial_local_follow_request_state(
                FollowRequestScenario::LocalFollower,
                target_locked,
            ));
            states.push(initial_local_follow_request_state(
                FollowRequestScenario::RemoteFollower,
                target_locked,
            ));
        }

        states
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        if local_follow_request_action_allowed(state) {
            actions.push(LocalFollowRequestAction::Authorize);
            actions.push(LocalFollowRequestAction::Reject);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        if !local_follow_request_action_allowed(state) {
            return None;
        }

        Some(match action {
            LocalFollowRequestAction::Authorize => authorize_local_follow_request(*state),
            LocalFollowRequestAction::Reject => reject_local_follow_request(*state),
        })
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "pending_local_follow_only_when_locked_local_follower",
                |_, state: &LocalFollowRequestState| {
                    state.local_follow != Some(LocalFollowState::Pending)
                        || (state.target_locked
                            && state.scenario == FollowRequestScenario::LocalFollower)
                },
            ),
            Property::always(
                "unlocked_local_follow_never_starts_pending",
                |_, state: &LocalFollowRequestState| {
                    state.local_follow != Some(LocalFollowState::Pending) || state.target_locked
                },
            ),
            Property::always(
                "queued_remote_request_only_when_locked_remote_follower",
                |_, state: &LocalFollowRequestState| {
                    state.remote_request != RemoteInboundFollowRequestState::Queued
                        || (state.target_locked
                            && state.scenario == FollowRequestScenario::RemoteFollower)
                },
            ),
            Property::always(
                "accepted_follow_has_no_pending_request_queue",
                |_, state: &LocalFollowRequestState| {
                    state.local_follow != Some(LocalFollowState::Accepted)
                        || state.remote_request != RemoteInboundFollowRequestState::Queued
                },
            ),
            Property::always(
                "fulfilled_remote_request_has_accepted_follow_row",
                |_, state: &LocalFollowRequestState| {
                    state.remote_request != RemoteInboundFollowRequestState::Fulfilled
                        || state.local_follow == Some(LocalFollowState::Accepted)
                },
            ),
            Property::sometimes(
                "pending_local_follow_reachable",
                |_, state: &LocalFollowRequestState| {
                    state.local_follow == Some(LocalFollowState::Pending)
                },
            ),
            Property::sometimes(
                "accepted_local_follow_reachable",
                |_, state: &LocalFollowRequestState| {
                    state.local_follow == Some(LocalFollowState::Accepted)
                },
            ),
            Property::sometimes(
                "queued_remote_request_reachable",
                |_, state: &LocalFollowRequestState| {
                    state.remote_request == RemoteInboundFollowRequestState::Queued
                },
            ),
        ]
    }
}

pub(crate) fn check_local_follow_request_model() {
    LocalFollowRequestModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_local_follow_request_model;

    #[test]
    fn local_follow_request_model_holds() {
        check_local_follow_request_model();
    }
}
