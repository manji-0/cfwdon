#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InboxActivityRecordState {
    Absent,
    InFlight,
    Processed,
}

/// What a delivery learns when it tries to claim an activity id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InboxClaim {
    /// This delivery applies the activity.
    Acquired,
    /// Another delivery holds the claim and has not finished. Its apply may
    /// have been cut off, so the sender must retry rather than stop.
    InFlight,
    /// The activity was applied before; the delivery is a replay.
    AlreadyProcessed,
}

/// Result of attempting to begin inbox processing for an activity ID.
pub fn inbox_activity_after_receive(
    current: InboxActivityRecordState,
) -> (InboxActivityRecordState, InboxClaim) {
    match current {
        InboxActivityRecordState::Absent => {
            (InboxActivityRecordState::InFlight, InboxClaim::Acquired)
        }
        InboxActivityRecordState::InFlight => (current, InboxClaim::InFlight),
        InboxActivityRecordState::Processed => (current, InboxClaim::AlreadyProcessed),
    }
}

pub fn inbox_activity_after_success(
    current: InboxActivityRecordState,
) -> Option<InboxActivityRecordState> {
    match current {
        InboxActivityRecordState::InFlight => Some(InboxActivityRecordState::Processed),
        InboxActivityRecordState::Absent | InboxActivityRecordState::Processed => None,
    }
}

pub fn inbox_activity_after_failure(
    current: InboxActivityRecordState,
) -> Option<InboxActivityRecordState> {
    match current {
        InboxActivityRecordState::InFlight => Some(InboxActivityRecordState::Absent),
        InboxActivityRecordState::Absent | InboxActivityRecordState::Processed => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_claim_distinguishes_in_flight_from_processed() {
        assert_eq!(
            inbox_activity_after_receive(InboxActivityRecordState::Absent),
            (InboxActivityRecordState::InFlight, InboxClaim::Acquired)
        );
        assert_eq!(
            inbox_activity_after_receive(InboxActivityRecordState::InFlight),
            (InboxActivityRecordState::InFlight, InboxClaim::InFlight)
        );
        assert_eq!(
            inbox_activity_after_receive(InboxActivityRecordState::Processed),
            (
                InboxActivityRecordState::Processed,
                InboxClaim::AlreadyProcessed
            )
        );
    }
}
