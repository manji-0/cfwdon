use cfwdon_domain::{
    InboxActivityRecordState, InboxClaim, inbox_activity_after_failure,
    inbox_activity_after_receive, inbox_activity_after_success,
};

use crate::inbox_replay::{InboxRecordOp, apply_record_op};
use crate::refinement::verify::assert_worker_refinement;

/// Mirrors the guard of each `inbox/activity_store.rs` statement: the claim
/// batch's INSERT OR IGNORE returns a row only for an absent row, and
/// `mark_inbox_activity_processed` / `release_inbox_activity_processing`
/// touch only an in-flight row.
fn worker_allows(state: InboxActivityRecordState, op: InboxRecordOp) -> bool {
    match op {
        InboxRecordOp::Receive => inbox_activity_after_receive(state).1 == InboxClaim::Acquired,
        InboxRecordOp::MarkProcessed => inbox_activity_after_success(state).is_some(),
        InboxRecordOp::Release => inbox_activity_after_failure(state).is_some(),
    }
}

fn worker_effect(state: InboxActivityRecordState, op: InboxRecordOp) -> InboxActivityRecordState {
    match op {
        InboxRecordOp::Receive if worker_allows(state, op) => InboxActivityRecordState::InFlight,
        InboxRecordOp::MarkProcessed if worker_allows(state, op) => {
            InboxActivityRecordState::Processed
        }
        InboxRecordOp::Release if worker_allows(state, op) => InboxActivityRecordState::Absent,
        InboxRecordOp::Receive | InboxRecordOp::MarkProcessed | InboxRecordOp::Release => state,
    }
}

pub(crate) fn check_inbox_replay_refinement() {
    assert_worker_refinement(
        [
            InboxActivityRecordState::Absent,
            InboxActivityRecordState::InFlight,
            InboxActivityRecordState::Processed,
        ],
        |_| {
            vec![
                InboxRecordOp::Receive,
                InboxRecordOp::MarkProcessed,
                InboxRecordOp::Release,
            ]
        },
        worker_allows,
        worker_effect,
        apply_record_op,
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
