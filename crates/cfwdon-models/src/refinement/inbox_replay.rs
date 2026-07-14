use stateright::Model;

use cfwdon_domain::{
    InboxActivityRecordState, inbox_activity_after_failure, inbox_activity_after_receive,
    inbox_activity_after_success,
};

use crate::inbox_replay::{InboxReplayAction, InboxReplayModel};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};

fn model_domain_step(
    state: InboxActivityRecordState,
    action: InboxReplayAction,
) -> Option<InboxActivityRecordState> {
    InboxReplayModel.next_state(&state, action)
}

/// Mirrors `begin_inbox_activity_processing`: INSERT OR IGNORE accepts only absent rows.
fn worker_allows_receive(state: InboxActivityRecordState) -> bool {
    inbox_activity_after_receive(state).1
}

/// Mirrors `mark_inbox_activity_processed` / `release_inbox_activity_processing` guards.
fn worker_allows_complete(state: InboxActivityRecordState, success: bool) -> bool {
    if success {
        inbox_activity_after_success(state).is_some()
    } else {
        inbox_activity_after_failure(state).is_some()
    }
}

fn worker_effect(
    state: InboxActivityRecordState,
    action: InboxReplayAction,
) -> InboxActivityRecordState {
    match action {
        InboxReplayAction::Receive => inbox_activity_after_receive(state).0,
        InboxReplayAction::CompleteSuccess => inbox_activity_after_success(state).unwrap_or(state),
        InboxReplayAction::CompleteFailure => inbox_activity_after_failure(state).unwrap_or(state),
    }
}

fn domain_step(
    state: InboxActivityRecordState,
    action: InboxReplayAction,
) -> InboxActivityRecordState {
    model_domain_step(state, action).unwrap_or(state)
}

pub(crate) fn check_inbox_replay_refinement() {
    assert_model_matches_domain(&InboxReplayModel, model_domain_step);

    assert_worker_refinement(
        InboxReplayModel.init_states(),
        |state| {
            let mut actions = vec![InboxReplayAction::Receive];
            if *state == InboxActivityRecordState::InFlight {
                actions.push(InboxReplayAction::CompleteSuccess);
                actions.push(InboxReplayAction::CompleteFailure);
            }
            actions
        },
        |state, action| match action {
            InboxReplayAction::Receive => worker_allows_receive(state),
            InboxReplayAction::CompleteSuccess => worker_allows_complete(state, true),
            InboxReplayAction::CompleteFailure => worker_allows_complete(state, false),
        },
        worker_effect,
        domain_step,
    );
}

#[cfg(test)]
mod tests {
    use super::check_inbox_replay_refinement;

    #[test]
    fn inbox_replay_refinement_holds() {
        check_inbox_replay_refinement();
    }
}
