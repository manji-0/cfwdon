//! Pure delivery retry and remote-follow reconciliation rules.

pub const DELIVERY_MAX_ATTEMPTS: u32 = 5;
pub const OUTBOX_DELIVERY_CONCURRENCY: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutboxDeliveryRecordState {
    Queued,
    Expanded,
    Delivered,
    Failed,
}

pub fn generic_outbox_has_follower_targets(follower_target_count: usize) -> bool {
    follower_target_count > 0
}

pub fn generic_outbox_parent_state_after_expand(
    follower_target_count: u32,
) -> OutboxDeliveryRecordState {
    if follower_target_count == 0 {
        OutboxDeliveryRecordState::Delivered
    } else {
        OutboxDeliveryRecordState::Expanded
    }
}

pub fn outbox_delivery_state_after_attempt(
    current: OutboxDeliveryRecordState,
    next_attempt: u32,
    outcome: DeliveryAttemptOutcome,
) -> OutboxDeliveryRecordState {
    match (current, outcome) {
        (_, DeliveryAttemptOutcome::Success) => OutboxDeliveryRecordState::Delivered,
        (OutboxDeliveryRecordState::Queued, DeliveryAttemptOutcome::Failure) => {
            if is_delivery_terminal(next_attempt) {
                OutboxDeliveryRecordState::Failed
            } else {
                OutboxDeliveryRecordState::Queued
            }
        }
        (terminal, _) => terminal,
    }
}

pub fn outbox_expand_slot_count(follower_target_count: u32, max_slots: u32) -> u32 {
    follower_target_count.min(max_slots)
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutboundDeliverySlot {
    pub state: OutboundActivityState,
    pub attempt_count: u32,
}

impl OutboundDeliverySlot {
    pub const fn queued() -> Self {
        Self {
            state: OutboundActivityState::Queued,
            attempt_count: 0,
        }
    }
}

pub fn outbound_delivery_slot_after_attempt(
    slot: OutboundDeliverySlot,
    outcome: DeliveryAttemptOutcome,
) -> OutboundDeliverySlot {
    if slot.state != OutboundActivityState::Queued {
        return slot;
    }
    let next_attempt = next_delivery_attempt_count(slot.attempt_count as i32);
    let mut next = OutboundDeliverySlot {
        state: slot.state,
        attempt_count: slot.attempt_count,
    };
    if matches!(outcome, DeliveryAttemptOutcome::Failure) {
        next.attempt_count = next_attempt;
    }
    next.state = outbound_state_after_delivery_attempt(slot.state, next_attempt, outcome);
    next
}

pub fn outbox_delivery_pool_size(total_slots: u8, max_concurrency: usize) -> u8 {
    total_slots.min(max_concurrency as u8)
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
    fn outbound_delivery_slot_transitions_match_retry_threshold() {
        let mut slot = OutboundDeliverySlot::queued();
        for _ in 0..(DELIVERY_MAX_ATTEMPTS - 1) {
            slot = outbound_delivery_slot_after_attempt(slot, DeliveryAttemptOutcome::Failure);
            assert_eq!(slot.state, OutboundActivityState::Queued);
        }
        slot = outbound_delivery_slot_after_attempt(slot, DeliveryAttemptOutcome::Failure);
        assert_eq!(slot.state, OutboundActivityState::Failed);
        assert_eq!(
            outbound_delivery_slot_after_attempt(
                OutboundDeliverySlot::queued(),
                DeliveryAttemptOutcome::Success,
            )
            .state,
            OutboundActivityState::Delivered
        );
    }

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
    fn generic_outbox_without_targets_completes_as_delivered() {
        assert_eq!(
            generic_outbox_parent_state_after_expand(0),
            OutboxDeliveryRecordState::Delivered
        );
        assert_eq!(
            generic_outbox_parent_state_after_expand(2),
            OutboxDeliveryRecordState::Expanded
        );
    }

    #[test]
    fn outbox_delivery_state_transitions_match_retry_threshold() {
        assert_eq!(
            outbox_delivery_state_after_attempt(
                OutboxDeliveryRecordState::Queued,
                DELIVERY_MAX_ATTEMPTS,
                DeliveryAttemptOutcome::Failure,
            ),
            OutboxDeliveryRecordState::Failed
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
