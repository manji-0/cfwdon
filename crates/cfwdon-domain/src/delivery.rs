//! Pure delivery retry and remote-follow reconciliation rules.

/// One attempt and Mastodon's 16 retries (`ActivityPub::DeliveryWorker`, `retry: 16`).
/// With [`delivery_backoff_seconds`] the retries span about two days.
pub const DELIVERY_MAX_ATTEMPTS: u32 = 17;
/// A remote `Retry-After` longer than this (12 hours) is shortened to it.
pub const RETRY_AFTER_CAP_SECONDS: u64 = 12 * 60 * 60;
pub const OUTBOX_DELIVERY_CONCURRENCY: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutboxDeliveryRecordState {
    Queued,
    /// Terminal for a generic row: its per-inbox rows were created and each
    /// one carries the payload and its own retry state from here on.
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
        (_, DeliveryAttemptOutcome::PermanentFailure) => OutboxDeliveryRecordState::Failed,
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
    /// Transient failure — retry with backoff until [`DELIVERY_MAX_ATTEMPTS`].
    Failure,
    /// Non-retryable remote rejection: a 4xx other than 401, 408 and 429, or a 501.
    PermanentFailure,
}

/// Classifies an HTTP status from a signed ActivityPub inbox POST.
///
/// Follows Mastodon's `response_error_unsalvageable?` (`JsonLdHelper`): a 4xx
/// other than 401, 408 and 429, or a 501, will not change by asking again.
/// A 401 stays retryable because the remote may not have fetched our key yet.
/// Any 2xx is success; everything else is retried.
pub fn delivery_disposition_for_http_status(status: u16) -> DeliveryAttemptOutcome {
    match status {
        200..=299 => DeliveryAttemptOutcome::Success,
        401 | 408 | 429 => DeliveryAttemptOutcome::Failure,
        400..=499 | 501 => DeliveryAttemptOutcome::PermanentFailure,
        _ => DeliveryAttemptOutcome::Failure,
    }
}

pub fn is_delivery_http_status_permanent(status: u16) -> bool {
    matches!(
        delivery_disposition_for_http_status(status),
        DeliveryAttemptOutcome::PermanentFailure
    )
}

/// Whether a retryable inbox answer's `Retry-After` header counts (429 or 503).
pub fn delivery_status_honors_retry_after(status: u16) -> bool {
    matches!(status, 429 | 503)
}

/// Reads a `Retry-After` value as seconds from `now_ms`: delta-seconds, or an
/// HTTP-date read by `parse_http_date_ms` (a past date is 0). `None` when the
/// value is neither.
pub fn retry_after_seconds(
    value: &str,
    now_ms: f64,
    parse_http_date_ms: impl FnOnce(&str) -> Option<f64>,
) -> Option<u64> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(value.parse().unwrap_or(u64::MAX));
    }
    // An HTTP-date names its day and month; a bare number a lenient date
    // parser would read as a year is not one.
    if !value.bytes().any(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let at_ms = parse_http_date_ms(value)?;
    Some(((at_ms - now_ms) / 1000.0).ceil().max(0.0) as u64)
}

/// Sidekiq's default retry delay, which Mastodon's `ActivityPub::DeliveryWorker`
/// uses: `count^4 + 15 + rand(10) * (count + 1)` seconds before retry `count`
/// (0 for the first). `random` is in [0, 1).
pub fn delivery_backoff_seconds(retry_count: u32, random: f64) -> u64 {
    let jitter = ((random * 10.0).floor() as u64).min(9);
    let count = u64::from(retry_count);
    count.pow(4) + 15 + jitter * (count + 1)
}

/// How long to wait after the failed attempt numbered `next_attempt` (1 for the
/// first attempt): the backoff for its retry, or the remote's `Retry-After`
/// when that is longer, capped at [`RETRY_AFTER_CAP_SECONDS`].
pub fn delivery_retry_delay_seconds(
    next_attempt: u32,
    random: f64,
    retry_after_seconds: Option<u64>,
) -> u64 {
    let backoff = delivery_backoff_seconds(next_attempt.saturating_sub(1), random);
    let asked = retry_after_seconds.map_or(0, |seconds| seconds.min(RETRY_AFTER_CAP_SECONDS));
    backoff.max(asked)
}

/// SQLite `datetime` modifier that moves a timestamp `delay_seconds` ahead.
pub fn delivery_retry_delay_modifier(delay_seconds: u64) -> String {
    format!("+{delay_seconds} seconds")
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
        (_, DeliveryAttemptOutcome::PermanentFailure) => OutboundActivityState::Failed,
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
    if matches!(
        outcome,
        DeliveryAttemptOutcome::Failure | DeliveryAttemptOutcome::PermanentFailure
    ) {
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
    fn retry_backoff_follows_sidekiq_schedule() {
        assert_eq!(delivery_retry_delay_seconds(1, 0.0, None), 15);
        assert_eq!(delivery_retry_delay_seconds(1, 0.999, None), 15 + 9);
        assert_eq!(delivery_retry_delay_seconds(2, 0.0, None), 16);
        assert_eq!(delivery_retry_delay_seconds(3, 0.55, None), 16 + 15 + 5 * 3);
        assert_eq!(
            delivery_retry_delay_seconds(DELIVERY_MAX_ATTEMPTS - 1, 0.0, None),
            15_u64.pow(4) + 15
        );
        assert_eq!(delivery_retry_delay_modifier(15), "+15 seconds");
    }

    #[test]
    fn retry_schedule_spans_about_two_days() {
        let fastest: u64 = (1..DELIVERY_MAX_ATTEMPTS)
            .map(|attempt| delivery_retry_delay_seconds(attempt, 0.0, None))
            .sum();
        let slowest: u64 = (1..DELIVERY_MAX_ATTEMPTS)
            .map(|attempt| delivery_retry_delay_seconds(attempt, 0.999, None))
            .sum();
        let day = 24 * 60 * 60;
        assert!((2 * day..3 * day).contains(&fastest), "{fastest}");
        assert!((2 * day..3 * day).contains(&slowest), "{slowest}");
    }

    #[test]
    fn retry_after_extends_backoff_up_to_cap() {
        assert_eq!(delivery_retry_delay_seconds(1, 0.0, Some(120)), 120);
        assert_eq!(delivery_retry_delay_seconds(1, 0.0, Some(3)), 15);
        assert_eq!(
            delivery_retry_delay_seconds(1, 0.0, Some(7 * 24 * 60 * 60)),
            RETRY_AFTER_CAP_SECONDS
        );
        assert!(delivery_status_honors_retry_after(429));
        assert!(delivery_status_honors_retry_after(503));
        assert!(!delivery_status_honors_retry_after(500));
    }

    #[test]
    fn retry_after_reads_delta_seconds_and_http_dates() {
        let now_ms = 1_000_000.0;
        let no_date = |_: &str| None;
        assert_eq!(retry_after_seconds(" 120 ", now_ms, no_date), Some(120));
        assert_eq!(retry_after_seconds("0", now_ms, no_date), Some(0));
        assert_eq!(retry_after_seconds("", now_ms, no_date), None);
        assert_eq!(retry_after_seconds("-5", now_ms, no_date), None);
        assert_eq!(retry_after_seconds("soon", now_ms, no_date), None);

        let date = "Wed, 21 Oct 2015 07:28:00 GMT";
        let in_90s = |value: &str| (value == date).then_some(now_ms + 89_500.0);
        assert_eq!(retry_after_seconds(date, now_ms, in_90s), Some(90));
        let past = |_: &str| Some(now_ms - 5_000.0);
        assert_eq!(retry_after_seconds(date, now_ms, past), Some(0));
        let reads_years = |_: &str| Some(now_ms + 1_000.0);
        assert_eq!(retry_after_seconds("1.5", now_ms, reads_years), None);
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
    fn permanent_http_failures_are_terminal_immediately() {
        assert_eq!(
            delivery_disposition_for_http_status(404),
            DeliveryAttemptOutcome::PermanentFailure
        );
        assert_eq!(
            delivery_disposition_for_http_status(410),
            DeliveryAttemptOutcome::PermanentFailure
        );
        for status in [400, 403, 404, 410, 422, 501] {
            assert_eq!(
                delivery_disposition_for_http_status(status),
                DeliveryAttemptOutcome::PermanentFailure,
                "HTTP {status}"
            );
        }
        for status in [401, 408, 429, 500, 502, 503, 504, 302] {
            assert_eq!(
                delivery_disposition_for_http_status(status),
                DeliveryAttemptOutcome::Failure,
                "HTTP {status}"
            );
        }
        assert_eq!(
            delivery_disposition_for_http_status(202),
            DeliveryAttemptOutcome::Success
        );
        assert!(is_delivery_http_status_permanent(403));
        assert!(!is_delivery_http_status_permanent(503));
        assert_eq!(
            outbox_delivery_state_after_attempt(
                OutboxDeliveryRecordState::Queued,
                1,
                DeliveryAttemptOutcome::PermanentFailure,
            ),
            OutboxDeliveryRecordState::Failed
        );
        assert_eq!(
            outbound_state_after_delivery_attempt(
                OutboundActivityState::Queued,
                1,
                DeliveryAttemptOutcome::PermanentFailure,
            ),
            OutboundActivityState::Failed
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
