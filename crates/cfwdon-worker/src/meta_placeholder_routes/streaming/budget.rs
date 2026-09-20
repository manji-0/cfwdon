use crate::snapshot_d1_request_metrics;

pub(super) const STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION: u32 = 90;

pub(super) const STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION: u32 = 200;

/// Cloudflare's per-invocation subrequest cap is 1000; recycle before D1 polling exhausts it.
pub(super) const STREAMING_MAX_D1_SUBREQUESTS_PER_INVOCATION: u32 = 900;

pub(super) fn streaming_poll_budget_exhausted(poll_rounds: u32, subscription_polls: u32) -> bool {
    poll_rounds >= STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION
        || subscription_polls >= STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION
        || snapshot_d1_request_metrics().query_count >= STREAMING_MAX_D1_SUBREQUESTS_PER_INVOCATION
}

pub(super) fn streaming_error_is_subrequest_limit(error: &worker::Error) -> bool {
    error
        .to_string()
        .contains("Too many API requests by single Worker invocation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{record_d1_query_duration, reset_d1_request_metrics};

    #[test]
    fn streaming_poll_budget_exhausts_before_cloudflare_subrequest_limit() {
        reset_d1_request_metrics();
        assert!(!streaming_poll_budget_exhausted(
            STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION - 1,
            STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION - 1
        ));
        assert!(streaming_poll_budget_exhausted(
            STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION,
            1
        ));
        assert!(streaming_poll_budget_exhausted(
            1,
            STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION
        ));
        reset_d1_request_metrics();
    }

    #[test]
    fn streaming_poll_budget_exhausts_on_d1_subrequest_count() {
        reset_d1_request_metrics();
        for _ in 0..STREAMING_MAX_D1_SUBREQUESTS_PER_INVOCATION {
            record_d1_query_duration(1);
        }
        assert!(streaming_poll_budget_exhausted(1, 1));
        reset_d1_request_metrics();
    }

    #[test]
    fn streaming_error_detects_cloudflare_subrequest_limit() {
        let error = worker::Error::RustError(
            "Error: Too many API requests by single Worker invocation".to_owned(),
        );

        assert!(streaming_error_is_subrequest_limit(&error));
    }
}
