//! Pure delivery retry and remote-follow reconciliation rules.

pub const DELIVERY_MAX_ATTEMPTS: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutboundActivityState {
    Queued,
    Delivered,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemoteFollowState {
    Pending,
    Accepted,
    Rejected,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FollowInboxResponse {
    Accept,
    Reject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeliveryAttemptOutcome {
    Success,
    Failure,
}

pub fn delivery_retry_delay_modifier(attempt: u32) -> &'static str {
    match attempt {
        1 => "+1 minute",
        2 => "+5 minutes",
        3 => "+15 minutes",
        _ => "+60 minutes",
    }
}

pub fn next_delivery_attempt_count(attempt_count: i32) -> u32 {
    attempt_count.saturating_add(1) as u32
}

pub fn is_delivery_terminal(next_attempt: u32) -> bool {
    next_attempt >= DELIVERY_MAX_ATTEMPTS
}

pub fn outbound_state_after_delivery_attempt(
    current: OutboundActivityState,
    next_attempt: u32,
    outcome: DeliveryAttemptOutcome,
) -> OutboundActivityState {
    match (current, outcome) {
        (_, DeliveryAttemptOutcome::Success) => OutboundActivityState::Delivered,
        (OutboundActivityState::Queued, DeliveryAttemptOutcome::Failure) => {
            if is_delivery_terminal(next_attempt) {
                OutboundActivityState::Failed
            } else {
                OutboundActivityState::Queued
            }
        }
        (terminal @ (OutboundActivityState::Delivered | OutboundActivityState::Failed), _) => {
            terminal
        }
    }
}

pub fn outbound_terminal_failure_follow_state(activity_type: &str) -> Option<RemoteFollowState> {
    match activity_type {
        "Follow" => Some(RemoteFollowState::Failed),
        _ => None,
    }
}

pub fn reconcile_pending_follow_on_outbound_terminal_failure(
    follow_state: RemoteFollowState,
    activity_type: &str,
) -> RemoteFollowState {
    if follow_state != RemoteFollowState::Pending {
        return follow_state;
    }
    outbound_terminal_failure_follow_state(activity_type).unwrap_or(follow_state)
}

pub fn initial_remote_follow_state(actor_locked: bool) -> RemoteFollowState {
    if actor_locked {
        RemoteFollowState::Pending
    } else {
        RemoteFollowState::Accepted
    }
}

pub fn follow_state_after_inbox_response(
    _current: RemoteFollowState,
    response: FollowInboxResponse,
) -> RemoteFollowState {
    match response {
        FollowInboxResponse::Accept => RemoteFollowState::Accepted,
        FollowInboxResponse::Reject => RemoteFollowState::Rejected,
    }
}

impl FollowInboxResponse {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accepted",
            Self::Reject => "rejected",
        }
    }
}

impl RemoteFollowState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Accepted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_retry_delay_modifier_steps_match_worker_schedule() {
        assert_eq!(delivery_retry_delay_modifier(1), "+1 minute");
        assert_eq!(delivery_retry_delay_modifier(2), "+5 minutes");
        assert_eq!(delivery_retry_delay_modifier(3), "+15 minutes");
        assert_eq!(delivery_retry_delay_modifier(4), "+60 minutes");
        assert_eq!(delivery_retry_delay_modifier(8), "+60 minutes");
    }

    #[test]
    fn terminal_failure_marks_follow_as_failed_only_for_follow() {
        assert_eq!(
            outbound_terminal_failure_follow_state("Follow"),
            Some(RemoteFollowState::Failed)
        );
        assert_eq!(outbound_terminal_failure_follow_state("Undo"), None);
        assert_eq!(outbound_terminal_failure_follow_state("Like"), None);
    }

    #[test]
    fn reconcile_only_updates_pending_follow() {
        assert_eq!(
            reconcile_pending_follow_on_outbound_terminal_failure(
                RemoteFollowState::Pending,
                "Follow",
            ),
            RemoteFollowState::Failed
        );
        assert_eq!(
            reconcile_pending_follow_on_outbound_terminal_failure(
                RemoteFollowState::Accepted,
                "Follow",
            ),
            RemoteFollowState::Accepted
        );
        assert_eq!(
            reconcile_pending_follow_on_outbound_terminal_failure(
                RemoteFollowState::Pending,
                "Undo",
            ),
            RemoteFollowState::Pending
        );
    }

    #[test]
    fn inbox_response_overwrites_current_follow_state() {
        assert_eq!(
            follow_state_after_inbox_response(
                RemoteFollowState::Pending,
                FollowInboxResponse::Accept,
            ),
            RemoteFollowState::Accepted
        );
        assert_eq!(
            follow_state_after_inbox_response(
                RemoteFollowState::Failed,
                FollowInboxResponse::Accept,
            ),
            RemoteFollowState::Accepted
        );
        assert_eq!(
            follow_state_after_inbox_response(
                RemoteFollowState::Pending,
                FollowInboxResponse::Reject,
            ),
            RemoteFollowState::Rejected
        );
    }

    #[test]
    fn outbound_state_transitions_match_retry_threshold() {
        assert_eq!(
            outbound_state_after_delivery_attempt(
                OutboundActivityState::Queued,
                1,
                DeliveryAttemptOutcome::Failure,
            ),
            OutboundActivityState::Queued
        );
        assert_eq!(
            outbound_state_after_delivery_attempt(
                OutboundActivityState::Queued,
                DELIVERY_MAX_ATTEMPTS,
                DeliveryAttemptOutcome::Failure,
            ),
            OutboundActivityState::Failed
        );
        assert_eq!(
            outbound_state_after_delivery_attempt(
                OutboundActivityState::Queued,
                1,
                DeliveryAttemptOutcome::Success,
            ),
            OutboundActivityState::Delivered
        );
    }
}
