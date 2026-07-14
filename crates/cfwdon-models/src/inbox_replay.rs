use cfwdon_domain::{
    InboxActivityRecordState, inbox_activity_after_failure, inbox_activity_after_receive,
    inbox_activity_after_success,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct InboxReplayModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum InboxReplayAction {
    Receive,
    CompleteSuccess,
    CompleteFailure,
}

impl Model for InboxReplayModel {
    type State = InboxActivityRecordState;
    type Action = InboxReplayAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![
            InboxActivityRecordState::Absent,
            InboxActivityRecordState::InFlight,
            InboxActivityRecordState::Processed,
        ]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        actions.push(InboxReplayAction::Receive);
        if *state == InboxActivityRecordState::InFlight {
            actions.push(InboxReplayAction::CompleteSuccess);
            actions.push(InboxReplayAction::CompleteFailure);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        match action {
            InboxReplayAction::Receive => {
                let (next, _) = inbox_activity_after_receive(*state);
                Some(next)
            }
            InboxReplayAction::CompleteSuccess => inbox_activity_after_success(*state),
            InboxReplayAction::CompleteFailure => inbox_activity_after_failure(*state),
        }
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "processed_replays_are_rejected",
                |_, state: &InboxActivityRecordState| {
                    if *state != InboxActivityRecordState::Processed {
                        return true;
                    }
                    let (_, accepted) = inbox_activity_after_receive(*state);
                    !accepted
                },
            ),
            Property::always(
                "in_flight_replays_are_rejected",
                |_, state: &InboxActivityRecordState| {
                    if *state != InboxActivityRecordState::InFlight {
                        return true;
                    }
                    let (next, accepted) = inbox_activity_after_receive(*state);
                    !accepted && next == InboxActivityRecordState::InFlight
                },
            ),
            Property::always(
                "success_only_from_in_flight",
                |_, state: &InboxActivityRecordState| {
                    inbox_activity_after_success(*state).is_none()
                        || *state == InboxActivityRecordState::InFlight
                },
            ),
            Property::always(
                "failure_only_releases_in_flight",
                |_, state: &InboxActivityRecordState| {
                    inbox_activity_after_failure(*state).is_none()
                        || *state == InboxActivityRecordState::InFlight
                },
            ),
            Property::sometimes(
                "processed_reachable",
                |_, state: &InboxActivityRecordState| *state == InboxActivityRecordState::Processed,
            ),
        ]
    }
}

pub(crate) fn check_inbox_replay_model() {
    InboxReplayModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_inbox_replay_model;

    #[test]
    fn inbox_replay_model_holds() {
        check_inbox_replay_model();
    }
}
