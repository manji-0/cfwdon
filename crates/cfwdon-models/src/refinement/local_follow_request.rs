use cfwdon_domain::{
    FollowRequestScenario, LocalFollowRequestState, LocalFollowState,
    RemoteInboundFollowRequestState, authorize_local_follow_request,
    initial_local_follow_request_state, initial_local_follow_state, reject_local_follow_request,
    remote_inbound_request_after_inbox_follow,
};

use crate::local_follow_request::{
    LocalFollowRequestAction, LocalFollowRequestModel, local_follow_request_action_allowed,
};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use stateright::Model;

fn model_domain_step(
    state: LocalFollowRequestState,
    action: LocalFollowRequestAction,
) -> Option<LocalFollowRequestState> {
    LocalFollowRequestModel.next_state(&state, action)
}

fn worker_allows(state: LocalFollowRequestState, action: LocalFollowRequestAction) -> bool {
    let _ = action;
    local_follow_request_action_allowed(&state)
}

fn worker_effect(
    state: LocalFollowRequestState,
    action: LocalFollowRequestAction,
) -> LocalFollowRequestState {
    if !worker_allows(state, action) {
        return state;
    }

    match action {
        LocalFollowRequestAction::Authorize => authorize_local_follow_request(state),
        LocalFollowRequestAction::Reject => reject_local_follow_request(state),
    }
}

fn domain_step(
    state: LocalFollowRequestState,
    action: LocalFollowRequestAction,
) -> LocalFollowRequestState {
    model_domain_step(state, action).unwrap_or(state)
}

pub(crate) fn check_local_follow_request_refinement() {
    assert_model_matches_domain(&LocalFollowRequestModel, model_domain_step);

    assert_worker_refinement(
        LocalFollowRequestModel.init_states(),
        |state| {
            if local_follow_request_action_allowed(state) {
                vec![
                    LocalFollowRequestAction::Authorize,
                    LocalFollowRequestAction::Reject,
                ]
            } else {
                Vec::new()
            }
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    assert_eq!(initial_local_follow_state(true), LocalFollowState::Pending);
    assert_eq!(
        initial_local_follow_state(false),
        LocalFollowState::Accepted
    );
    assert_eq!(
        remote_inbound_request_after_inbox_follow(true),
        RemoteInboundFollowRequestState::Queued
    );
    assert_eq!(
        remote_inbound_request_after_inbox_follow(false),
        RemoteInboundFollowRequestState::Fulfilled
    );

    let local_pending =
        initial_local_follow_request_state(FollowRequestScenario::LocalFollower, true);
    assert_eq!(
        authorize_local_follow_request(local_pending).local_follow,
        Some(LocalFollowState::Accepted)
    );
    assert_eq!(
        reject_local_follow_request(local_pending).local_follow,
        None
    );

    let remote_queued =
        initial_local_follow_request_state(FollowRequestScenario::RemoteFollower, true);
    let authorized = authorize_local_follow_request(remote_queued);
    assert_eq!(authorized.local_follow, Some(LocalFollowState::Accepted));
    assert_eq!(
        authorized.remote_request,
        RemoteInboundFollowRequestState::Fulfilled
    );
    assert_eq!(
        reject_local_follow_request(remote_queued).remote_request,
        RemoteInboundFollowRequestState::Absent
    );
}

#[cfg(test)]
mod tests {
    use super::check_local_follow_request_refinement;

    #[test]
    fn local_follow_request_refinement_holds() {
        check_local_follow_request_refinement();
    }
}
