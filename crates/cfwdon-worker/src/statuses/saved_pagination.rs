//! Cursor paging for the viewer's favourites and bookmarks. Mastodon pages
//! these by the favourite / bookmark record id rather than the status id; the
//! SQLite rowid of the `favourites` / `bookmarks` row plays that role here.

use crate::db_utils::d1_results;
use crate::request_utils::{InternalPaginationIds, build_internal_cursor_link_for_url_with_min_id};
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;
use worker::d1::D1Type;
use worker::{Request, Result};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct SavedStatusesQuery {
    pub(crate) limit: Option<u32>,
    pub(crate) max_id: Option<String>,
    pub(crate) since_id: Option<String>,
    pub(crate) min_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SavedStatusesPage {
    pub(crate) limit: u32,
    pub(crate) max_id: Option<i64>,
    pub(crate) since_id: Option<i64>,
    pub(crate) min_id: Option<i64>,
}

impl SavedStatusesPage {
    /// `None` when a cursor matches no row; see [`InternalPaginationIds::parse`].
    pub(crate) fn from_request(req: &Request) -> Option<Self> {
        let query: SavedStatusesQuery = req.query().unwrap_or_default();
        let InternalPaginationIds {
            max_id,
            since_id,
            min_id,
        } = InternalPaginationIds::parse(
            query.max_id.as_deref(),
            query.since_id.as_deref(),
            query.min_id.as_deref(),
        )?;
        Some(Self {
            limit: query.limit.unwrap_or(20).clamp(1, 40),
            max_id,
            since_id,
            min_id,
        })
    }

    /// `min_id` pages forward from the cursor (oldest first); everything else
    /// pages backward from the newest match.
    pub(crate) fn walks_forward(&self) -> bool {
        self.min_id.is_some()
    }
}

/// Rows of `table` saved by `account_id`, newest first, for one page.
/// Over-fetches so rows whose status the viewer can no longer see can be
/// dropped without leaving the page short.
pub(crate) async fn list_saved_status_rows<T: DeserializeOwned>(
    db: &D1Database,
    table: &'static str,
    account_id: &str,
    page: SavedStatusesPage,
) -> Result<Vec<T>> {
    let cursor = |value: Option<i64>| {
        value
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null)
    };
    let bindings = [
        D1Type::Text(account_id),
        cursor(page.max_id),
        cursor(page.min_id.or(page.since_id)),
        D1Type::Integer(page.limit.saturating_mul(3) as i32),
    ];
    let order = if page.walks_forward() { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT rowid AS cursor_id, status_id, remote_status_id
         FROM {table}
         WHERE account_id = ?1
           AND (?2 IS NULL OR rowid < ?2)
           AND (?3 IS NULL OR rowid > ?3)
         ORDER BY rowid {order}
         LIMIT ?4"
    );
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
    let mut rows = d1_results::<T>(&result)?;
    if page.walks_forward() {
        rows.reverse();
    }
    Ok(rows)
}

/// Mastodon emits `next` (older) and `prev` (newer) links whenever a page has
/// results; `first` / `last` are the newest and oldest cursors on the page.
pub(crate) fn saved_statuses_link_header(
    url: &Url,
    limit: u32,
    first: Option<i64>,
    last: Option<i64>,
) -> Result<Option<String>> {
    let (Some(first), Some(last)) = (first, last) else {
        return Ok(None);
    };
    Ok(Some(format!(
        "{}, {}",
        build_internal_cursor_link_for_url_with_min_id(url, limit, Some(last), None, None, "next")?,
        build_internal_cursor_link_for_url_with_min_id(
            url,
            limit,
            None,
            None,
            Some(first),
            "prev"
        )?,
    )))
}

#[cfg(test)]
mod tests {
    use super::saved_statuses_link_header;

    #[test]
    fn link_header_pages_older_by_max_id_and_newer_by_min_id() {
        let url =
            url::Url::parse("https://example.com/api/v1/favourites?limit=2&max_id=9").unwrap();
        let header = saved_statuses_link_header(&url, 2, Some(8), Some(5))
            .unwrap()
            .unwrap();
        assert_eq!(
            header,
            "<https://example.com/api/v1/favourites?limit=2&max_id=5>; rel=\"next\", \
             <https://example.com/api/v1/favourites?limit=2&min_id=8>; rel=\"prev\""
        );
    }

    #[test]
    fn empty_page_has_no_link_header() {
        let url = url::Url::parse("https://example.com/api/v1/bookmarks").unwrap();
        assert_eq!(
            saved_statuses_link_header(&url, 20, None, None).unwrap(),
            None
        );
    }
}
