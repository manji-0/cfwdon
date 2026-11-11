//! Prunes federation bookkeeping rows that reached a terminal state longer
//! ago than `FEDERATION_ACTIVITY_RETENTION_DAYS`.
//!
//! Only terminal rows go: deliveries that are `delivered` or `failed`, and
//! inbox activities with a `processed_at`. Queued, `expanded` (generic rows
//! whose per-inbox rows may still be pending), and `in_flight` rows stay. No
//! foreign key points at these tables.

use crate::observability::log_federation_event;
use crate::tracked_d1::D1Database;
use worker::Result;
use worker::d1::D1Type;

/// Rows per `DELETE`, kept small so one statement stays well inside D1 limits.
const RETENTION_BATCH_SIZE: u32 = 500;
/// Batches per table per cron tick; a backlog drains over later ticks.
const RETENTION_MAX_BATCHES_PER_TABLE: u32 = 10;

struct RetentionTable {
    name: &'static str,
    /// `?1` is the `datetime` age modifier, `?2` the batch size.
    delete_sql: &'static str,
}

const RETENTION_TABLES: [RetentionTable; 3] = [
    RetentionTable {
        name: "outbox_deliveries",
        delete_sql: "DELETE FROM outbox_deliveries
             WHERE id IN (
                 SELECT id
                 FROM outbox_deliveries
                 WHERE state IN ('delivered', 'failed')
                   AND updated_at < datetime(CURRENT_TIMESTAMP, ?1)
                 LIMIT ?2
             )",
    },
    RetentionTable {
        name: "outbound_activities",
        delete_sql: "DELETE FROM outbound_activities
             WHERE id IN (
                 SELECT id
                 FROM outbound_activities
                 WHERE state IN ('delivered', 'failed')
                   AND updated_at < datetime(CURRENT_TIMESTAMP, ?1)
                 LIMIT ?2
             )",
    },
    RetentionTable {
        name: "inbox_activities",
        delete_sql: "DELETE FROM inbox_activities
             WHERE rowid IN (
                 SELECT rowid
                 FROM inbox_activities
                 WHERE processed_at < datetime(CURRENT_TIMESTAMP, ?1)
                 LIMIT ?2
             )",
    },
];

fn retention_age_modifier(retention_days: u32) -> String {
    format!("-{retention_days} days")
}

async fn prune_table(db: &D1Database, table: &RetentionTable, modifier: &str) -> Result<u32> {
    let bindings = [
        D1Type::Text(modifier),
        D1Type::Integer(RETENTION_BATCH_SIZE as i32),
    ];
    let mut deleted = 0;
    for _ in 0..RETENTION_MAX_BATCHES_PER_TABLE {
        let result = db
            .prepare(table.delete_sql)
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        let changes = result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) as u32;
        deleted += changes;
        if changes < RETENTION_BATCH_SIZE {
            break;
        }
    }
    Ok(deleted)
}

/// One cron pass; `None` retention keeps every row.
pub(crate) async fn prune_terminal_federation_activities(
    db: &D1Database,
    retention_days: Option<u32>,
) -> Result<()> {
    let Some(retention_days) = retention_days else {
        return Ok(());
    };
    let modifier = retention_age_modifier(retention_days);
    let mut counts = serde_json::Map::new();
    for table in &RETENTION_TABLES {
        let deleted = prune_table(db, table, &modifier).await?;
        counts.insert(table.name.to_owned(), deleted.into());
    }
    if counts.values().any(|deleted| deleted.as_u64() != Some(0)) {
        log_federation_event(
            "federation_retention_pruned",
            "ok",
            format!("pruned terminal federation activity rows: {counts:?}"),
            serde_json::json!({ "retention_days": retention_days, "deleted": counts }),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::retention_age_modifier;

    #[test]
    fn retention_modifier_is_a_sqlite_day_offset() {
        assert_eq!(retention_age_modifier(30), "-30 days");
    }
}
