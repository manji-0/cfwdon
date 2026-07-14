#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InboxActivityRecordState {
    Absent,
    InFlight,
    Processed,
}

/// Result of attempting to begin inbox processing for an activity ID.
pub fn inbox_activity_after_receive(
    current: InboxActivityRecordState,
) -> (InboxActivityRecordState, bool) {
    match current {
        InboxActivityRecordState::Absent => (InboxActivityRecordState::InFlight, true),
        InboxActivityRecordState::InFlight | InboxActivityRecordState::Processed => {
            (current, false)
        }
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
    fn replay_is_rejected_while_in_flight_or_processed() {
        assert_eq!(
            inbox_activity_after_receive(InboxActivityRecordState::InFlight),
            (InboxActivityRecordState::InFlight, false)
        );
        assert_eq!(
            inbox_activity_after_receive(InboxActivityRecordState::Processed),
            (InboxActivityRecordState::Processed, false)
        );
    }
}
