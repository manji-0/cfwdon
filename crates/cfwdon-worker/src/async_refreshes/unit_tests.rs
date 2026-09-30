use crate::async_refreshes::{context_async_refresh_id, format_async_refresh_header_value};

#[test]
fn context_async_refresh_id_uses_context_namespace() {
    assert_eq!(
        context_async_refresh_id("status-123"),
        "context:status-123:refresh"
    );
}

#[test]
fn format_async_refresh_header_value_includes_retry_and_result_count() {
    assert_eq!(
        format_async_refresh_header_value("context:status-123:refresh", 3, Some(0)),
        "id=\"context:status-123:refresh\", retry=3, result_count=0"
    );
}
