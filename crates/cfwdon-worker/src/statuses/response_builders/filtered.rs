use super::super::{AccountFilterMatcher, load_status_filtered};
use worker::Result;

use crate::D1Database;

pub(super) async fn filtered_status_for_viewer(
    db: &D1Database,
    filter_matcher: Option<&AccountFilterMatcher>,
    account_id: &str,
    status_id: &str,
    text: &str,
    spoiler_text: &str,
) -> Result<Vec<serde_json::Value>> {
    if let Some(filter_matcher) = filter_matcher {
        return Ok(filter_matcher.filtered_status(status_id, text, spoiler_text));
    }

    load_status_filtered(db, account_id, status_id, text, spoiler_text).await
}
