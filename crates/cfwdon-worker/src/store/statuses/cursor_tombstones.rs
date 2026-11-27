//! Timeline positions of deleted statuses, so a paging cursor that names one
//! still resolves. See `migrations/139_timeline_cursor_tombstones.sql`.

use crate::tracked_d1::D1Database;
use serde::Deserialize;
use worker::{Result, d1::D1Type};

/// How long a deleted status still works as a paging cursor.
const TOMBSTONE_RETENTION: &str = "-7 days";

/// Records the local status `?1` before it is deleted. Bind the status id.
pub(crate) const RECORD_LOCAL_STATUS_TOMBSTONE_SQL: &str =
    "INSERT OR REPLACE INTO timeline_cursor_tombstones (id, timestamp)
     SELECT id, created_at
     FROM statuses
     WHERE id = ?1";

/// Records every boost wrapper of `?1` (account) for `?2` (target uri) before
/// they are deleted.
pub(crate) const RECORD_REBLOG_WRAPPER_TOMBSTONES_SQL: &str =
    "INSERT OR REPLACE INTO timeline_cursor_tombstones (id, timestamp)
     SELECT id, created_at
     FROM statuses
     WHERE account_id = ?1
       AND boost_of_uri = ?2";

/// Records the remote status `?1` before it is deleted. Bind the status id.
pub(crate) const RECORD_REMOTE_STATUS_TOMBSTONE_SQL: &str =
    "INSERT OR REPLACE INTO timeline_cursor_tombstones (id, timestamp)
     SELECT id, published_at
     FROM remote_statuses
     WHERE id = ?1";

/// Timeline timestamp of a status id: a live local or remote status, else a
/// recently deleted one. Live rows win over a tombstone with the same id.
const CURSOR_TIMESTAMP_SQL: &str = "SELECT timestamp
     FROM (
        SELECT 0 AS rank, created_at AS timestamp FROM statuses WHERE id = ?1
        UNION ALL
        SELECT 1 AS rank, published_at AS timestamp FROM remote_statuses WHERE id = ?1
        UNION ALL
        SELECT 2 AS rank, timestamp FROM timeline_cursor_tombstones WHERE id = ?1
     )
     ORDER BY rank
     LIMIT 1";

const PURGE_TOMBSTONES_SQL: &str = "DELETE FROM timeline_cursor_tombstones
     WHERE deleted_at < datetime('now', ?1)";

#[derive(Debug, Deserialize)]
struct CursorTimestampRow {
    timestamp: String,
}

pub(crate) async fn find_timeline_cursor_timestamp(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<String>> {
    let binding = D1Type::Text(status_id);
    Ok(db
        .prepare(CURSOR_TIMESTAMP_SQL)
        .bind_refs(&binding)?
        .first::<CursorTimestampRow>(None)
        .await?
        .map(|row| row.timestamp))
}

pub(crate) async fn purge_expired_timeline_cursor_tombstones(db: &D1Database) -> Result<()> {
    let modifier = D1Type::Text(TOMBSTONE_RETENTION);
    db.prepare(PURGE_TOMBSTONES_SQL)
        .bind_refs(&modifier)?
        .run()
        .await?;
    Ok(())
}
