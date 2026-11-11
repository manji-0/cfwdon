//! One activity id delivered to the inbox, with the remote retrying until it
//! gets a 2xx, a Worker that may be cut off mid-apply, and the 15-minute
//! stale-claim reclaim. The record transitions go through the domain
//! functions; the remote's reaction to each claim mirrors
//! `process_verified_inbox_activity`.

use std::collections::{HashMap, HashSet};

use cfwdon_domain::{
    InboxActivityRecordState, InboxClaim, inbox_activity_after_failure,
    inbox_activity_after_receive, inbox_activity_after_success,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct InboxReplayModel {
    /// The answer to an `InFlight` claim. `true` reproduces the pre-fix
    /// worker, which answered every duplicate 202.
    pub(crate) in_flight_answered_as_replay: bool,
}

pub(crate) const INBOX_REPLAY_MODEL: InboxReplayModel = InboxReplayModel {
    in_flight_answered_as_replay: false,
};

/// Who holds the in-flight row. A live apply is assumed to finish well inside
/// the 15-minute stale window, so only a cut-off claim goes stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ClaimHolder {
    Nobody,
    Applying,
    CutOff,
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RemoteSender {
    /// No 2xx yet: the remote will deliver again.
    Retrying,
    /// Got a 2xx: the remote never sends this activity again.
    Accepted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct InboxReplayState {
    pub(crate) record: InboxActivityRecordState,
    pub(crate) holder: ClaimHolder,
    /// The activity's effect is visible (ghost variable).
    pub(crate) applied: bool,
    pub(crate) remote: RemoteSender,
    /// The claim the last action's delivery saw, if that action was a delivery.
    pub(crate) last_claim: Option<InboxClaim>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum InboxReplayAction {
    Deliver,
    ApplySucceeds,
    ApplyFails,
    CutOffBeforeEffect,
    CutOffAfterEffect,
    ClaimGoesStale,
    CronReclaim,
}

/// Row operations the worker runs, each a domain transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum InboxRecordOp {
    Receive,
    MarkProcessed,
    Release,
}

pub(crate) fn apply_record_op(
    record: InboxActivityRecordState,
    op: InboxRecordOp,
) -> InboxActivityRecordState {
    match op {
        InboxRecordOp::Receive => inbox_activity_after_receive(record).0,
        InboxRecordOp::MarkProcessed => inbox_activity_after_success(record).unwrap_or(record),
        InboxRecordOp::Release => inbox_activity_after_failure(record).unwrap_or(record),
    }
}

/// The row operations one model action runs, in order.
pub(crate) fn record_ops(
    state: &InboxReplayState,
    action: InboxReplayAction,
) -> Vec<InboxRecordOp> {
    match action {
        InboxReplayAction::Deliver if state.holder == ClaimHolder::Stale => {
            vec![InboxRecordOp::Release, InboxRecordOp::Receive]
        }
        InboxReplayAction::Deliver => vec![InboxRecordOp::Receive],
        InboxReplayAction::ApplySucceeds => vec![InboxRecordOp::MarkProcessed],
        InboxReplayAction::ApplyFails => vec![InboxRecordOp::Release],
        InboxReplayAction::CronReclaim if state.applied => vec![InboxRecordOp::MarkProcessed],
        InboxReplayAction::CronReclaim => vec![InboxRecordOp::Release],
        InboxReplayAction::CutOffBeforeEffect
        | InboxReplayAction::CutOffAfterEffect
        | InboxReplayAction::ClaimGoesStale => Vec::new(),
    }
}

impl InboxReplayModel {
    fn remote_after_claim(&self, claim: InboxClaim, remote: RemoteSender) -> RemoteSender {
        match claim {
            InboxClaim::Acquired => remote,
            InboxClaim::InFlight if self.in_flight_answered_as_replay => RemoteSender::Accepted,
            InboxClaim::InFlight => RemoteSender::Retrying,
            InboxClaim::AlreadyProcessed => RemoteSender::Accepted,
        }
    }
}

impl Model for InboxReplayModel {
    type State = InboxReplayState;
    type Action = InboxReplayAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![InboxReplayState {
            record: InboxActivityRecordState::Absent,
            holder: ClaimHolder::Nobody,
            applied: false,
            remote: RemoteSender::Retrying,
            last_claim: None,
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        if state.remote == RemoteSender::Retrying {
            actions.push(InboxReplayAction::Deliver);
        }
        match state.holder {
            ClaimHolder::Nobody => {}
            ClaimHolder::Applying => actions.extend([
                InboxReplayAction::ApplySucceeds,
                InboxReplayAction::ApplyFails,
                InboxReplayAction::CutOffBeforeEffect,
                InboxReplayAction::CutOffAfterEffect,
            ]),
            ClaimHolder::CutOff => actions.push(InboxReplayAction::ClaimGoesStale),
            ClaimHolder::Stale => actions.push(InboxReplayAction::CronReclaim),
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;
        next.last_claim = None;
        for op in record_ops(state, action) {
            if op == InboxRecordOp::Receive {
                next.last_claim = Some(inbox_activity_after_receive(next.record).1);
            }
            next.record = apply_record_op(next.record, op);
        }
        match action {
            InboxReplayAction::Deliver => {
                let claim = next.last_claim?;
                next.remote = self.remote_after_claim(claim, state.remote);
                if claim == InboxClaim::Acquired {
                    next.holder = ClaimHolder::Applying;
                }
            }
            InboxReplayAction::ApplySucceeds => {
                next.applied = true;
                next.holder = ClaimHolder::Nobody;
                next.remote = RemoteSender::Accepted;
            }
            InboxReplayAction::ApplyFails | InboxReplayAction::CronReclaim => {
                next.holder = ClaimHolder::Nobody;
            }
            InboxReplayAction::CutOffBeforeEffect => next.holder = ClaimHolder::CutOff,
            InboxReplayAction::CutOffAfterEffect => {
                next.applied = true;
                next.holder = ClaimHolder::CutOff;
            }
            InboxReplayAction::ClaimGoesStale => next.holder = ClaimHolder::Stale,
        }
        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "accepted_delivery_was_applied",
                |_, state: &InboxReplayState| {
                    state.remote != RemoteSender::Accepted || state.applied
                },
            ),
            Property::always(
                "in_flight_duplicate_is_not_accepted",
                |_, state: &InboxReplayState| {
                    state.last_claim != Some(InboxClaim::InFlight)
                        || state.remote == RemoteSender::Retrying
                },
            ),
            Property::always(
                "claim_held_exactly_while_in_flight",
                |_, state: &InboxReplayState| {
                    (state.record == InboxActivityRecordState::InFlight)
                        == (state.holder != ClaimHolder::Nobody)
                },
            ),
            Property::always("processed_row_has_effect", |_, state: &InboxReplayState| {
                state.record != InboxActivityRecordState::Processed || state.applied
            }),
            Property::sometimes(
                "processed_replay_answered",
                |_, state: &InboxReplayState| {
                    state.last_claim == Some(InboxClaim::AlreadyProcessed)
                },
            ),
            Property::sometimes(
                "in_flight_duplicate_answered",
                |_, state: &InboxReplayState| state.last_claim == Some(InboxClaim::InFlight),
            ),
        ]
    }
}

/// Liveness under a remote that keeps retrying: from every reachable state, a
/// path still leads to the effect applied and the remote done. Stateright's
/// `eventually` only inspects terminal paths, and this model's retry loops
/// have none, so this walks the state graph instead.
pub(crate) fn unrecoverable_states(model: &InboxReplayModel) -> Vec<InboxReplayState> {
    let mut successors: HashMap<InboxReplayState, Vec<InboxReplayState>> = HashMap::new();
    let mut stack = model.init_states();
    while let Some(state) = stack.pop() {
        if successors.contains_key(&state) {
            continue;
        }
        let mut actions = Vec::new();
        model.actions(&state, &mut actions);
        let next: Vec<_> = actions
            .into_iter()
            .filter_map(|action| model.next_state(&state, action))
            .collect();
        stack.extend(next.iter().copied());
        successors.insert(state, next);
    }

    let mut recoverable: HashSet<InboxReplayState> = successors
        .keys()
        .filter(|state| state.applied && state.remote == RemoteSender::Accepted)
        .copied()
        .collect();
    loop {
        let before = recoverable.len();
        for (state, next) in &successors {
            if next.iter().any(|candidate| recoverable.contains(candidate)) {
                recoverable.insert(*state);
            }
        }
        if recoverable.len() == before {
            break;
        }
    }
    successors
        .into_keys()
        .filter(|state| !recoverable.contains(state))
        .collect()
}

pub(crate) fn check_inbox_replay_model() {
    INBOX_REPLAY_MODEL
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
    let stuck = unrecoverable_states(&INBOX_REPLAY_MODEL);
    assert!(
        stuck.is_empty(),
        "inbox activity can no longer be applied from {stuck:?}"
    );
}

#[cfg(test)]
mod tests {
    use stateright::{Checker, Model};

    use super::{InboxReplayModel, check_inbox_replay_model, unrecoverable_states};

    #[test]
    fn inbox_replay_model_holds() {
        check_inbox_replay_model();
    }

    #[test]
    fn answering_in_flight_as_replay_loses_cut_off_activity() {
        let pre_fix = InboxReplayModel {
            in_flight_answered_as_replay: true,
        };
        let checker = pre_fix.checker().spawn_dfs().join();
        assert!(checker.discovery("accepted_delivery_was_applied").is_some());
        assert!(
            checker
                .discovery("in_flight_duplicate_is_not_accepted")
                .is_some()
        );
        assert!(!unrecoverable_states(&pre_fix).is_empty());
    }
}
