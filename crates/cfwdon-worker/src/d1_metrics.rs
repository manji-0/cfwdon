use std::cell::RefCell;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct D1RequestMetrics {
    pub query_count: u32,
    pub sql_ms_sum: u64,
}

thread_local! {
    static D1_REQUEST_METRICS: RefCell<D1RequestMetrics> =
        RefCell::new(D1RequestMetrics::default());
}

pub(crate) fn reset_d1_request_metrics() {
    D1_REQUEST_METRICS.with(|metrics| *metrics.borrow_mut() = D1RequestMetrics::default());
}

pub(crate) fn snapshot_d1_request_metrics() -> D1RequestMetrics {
    D1_REQUEST_METRICS.with(|metrics| *metrics.borrow())
}

pub(crate) fn record_d1_query_duration(duration_ms: u64) {
    D1_REQUEST_METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        metrics.query_count = metrics.query_count.saturating_add(1);
        metrics.sql_ms_sum = metrics.sql_ms_sum.saturating_add(duration_ms);
    });
}

pub(crate) fn record_d1_wall_clock(started_at_ms: f64) {
    let duration_ms = (js_sys::Date::now() - started_at_ms).max(0.0).round() as u64;
    record_d1_query_duration(duration_ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_metrics_accumulate_and_reset() {
        reset_d1_request_metrics();
        record_d1_query_duration(12);
        record_d1_query_duration(8);
        assert_eq!(
            snapshot_d1_request_metrics(),
            D1RequestMetrics {
                query_count: 2,
                sql_ms_sum: 20,
            }
        );
        reset_d1_request_metrics();
        assert_eq!(snapshot_d1_request_metrics(), D1RequestMetrics::default());
    }
}
