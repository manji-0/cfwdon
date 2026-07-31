//! Seekable timeline cursor SQL helpers.
//!
//! SQLite cannot use disjunctions like `? IS NULL OR ts < ?` as index range
//! constraints. These helpers emit bounds only when a cursor is present.

use super::ResolvedTimelineCursor;
use worker::d1::D1Type;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedTimelineCursorSlots {
    pub max_timestamp: Option<usize>,
    pub max_id: Option<usize>,
    pub min_timestamp: Option<usize>,
    pub min_id: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MinTimestampCursorSlots {
    pub timestamp: usize,
    pub id: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct StatusIdCursorParts {
    pub with_clauses: Vec<String>,
    pub predicates: Vec<String>,
}

/// Append resolved cursor bindings after any leading parameters. Slot numbers are 1-based.
pub(crate) fn append_resolved_timeline_cursor_bindings<'a>(
    bindings: &mut Vec<D1Type<'a>>,
    cursor: &'a ResolvedTimelineCursor,
) -> ResolvedTimelineCursorSlots {
    append_timeline_cursor_bindings(
        bindings,
        cursor.max_timestamp.as_deref(),
        cursor.max_id.as_deref(),
        cursor.min_timestamp.as_deref(),
        cursor.min_id.as_deref(),
    )
}

pub(crate) fn append_timeline_cursor_bindings<'a>(
    bindings: &mut Vec<D1Type<'a>>,
    max_timestamp: Option<&'a str>,
    max_id: Option<&'a str>,
    min_timestamp: Option<&'a str>,
    min_id: Option<&'a str>,
) -> ResolvedTimelineCursorSlots {
    let mut slots = ResolvedTimelineCursorSlots::default();

    if let (Some(timestamp), Some(id)) = (max_timestamp, max_id) {
        bindings.push(D1Type::Text(timestamp));
        slots.max_timestamp = Some(bindings.len());
        bindings.push(D1Type::Text(id));
        slots.max_id = Some(bindings.len());
    }

    if let (Some(timestamp), Some(id)) = (min_timestamp, min_id) {
        bindings.push(D1Type::Text(timestamp));
        slots.min_timestamp = Some(bindings.len());
        bindings.push(D1Type::Text(id));
        slots.min_id = Some(bindings.len());
    }

    slots
}

pub(crate) fn append_min_timestamp_cursor_bindings<'a>(
    bindings: &mut Vec<D1Type<'a>>,
    min_timestamp: &'a str,
    min_id: Option<&'a str>,
) -> MinTimestampCursorSlots {
    bindings.push(D1Type::Text(min_timestamp));
    let timestamp_slot = bindings.len();
    let id_slot = min_id.map(|id| {
        bindings.push(D1Type::Text(id));
        bindings.len()
    });
    MinTimestampCursorSlots {
        timestamp: timestamp_slot,
        id: id_slot,
    }
}

pub(crate) fn seekable_resolved_timeline_cursor_predicates(
    timestamp_column: &str,
    id_column: &str,
    slots: &ResolvedTimelineCursorSlots,
) -> String {
    let mut predicates = String::new();
    if let (Some(timestamp_slot), Some(id_slot)) = (slots.max_timestamp, slots.max_id) {
        predicates.push_str(&format!(
            "
               AND {timestamp_column} <= ?{timestamp_slot}
               AND ({timestamp_column} < ?{timestamp_slot} OR {id_column} < ?{id_slot})"
        ));
    }
    if let (Some(timestamp_slot), Some(id_slot)) = (slots.min_timestamp, slots.min_id) {
        predicates.push_str(&format!(
            "
               AND {timestamp_column} >= ?{timestamp_slot}
               AND ({timestamp_column} > ?{timestamp_slot} OR {id_column} > ?{id_slot})"
        ));
    }
    predicates
}

pub(crate) fn seekable_min_timestamp_cursor_predicates(
    timestamp_column: &str,
    id_column: &str,
    slots: &MinTimestampCursorSlots,
) -> String {
    match slots.id {
        Some(id_slot) => format!(
            "
               AND (
                    {timestamp_column} > ?{timestamp}
                    OR ({timestamp_column} = ?{timestamp} AND {id_column} > ?{id_slot})
               )",
            timestamp = slots.timestamp,
            id_slot = id_slot,
        ),
        None => format!(
            "
               AND {timestamp_column} > ?{timestamp}",
            timestamp = slots.timestamp,
        ),
    }
}

pub(crate) fn append_local_status_id_cursor_parts<'a>(
    bindings: &mut Vec<D1Type<'a>>,
    table_alias: &str,
    max_id: Option<&'a str>,
    min_id: Option<&'a str>,
) -> StatusIdCursorParts {
    append_status_id_cursor_parts(
        bindings,
        "statuses",
        "created_at",
        table_alias,
        max_id,
        min_id,
    )
}

pub(crate) fn append_remote_status_id_cursor_parts<'a>(
    bindings: &mut Vec<D1Type<'a>>,
    table_alias: &str,
    max_id: Option<&'a str>,
    min_id: Option<&'a str>,
) -> StatusIdCursorParts {
    append_status_id_cursor_parts(
        bindings,
        "remote_statuses",
        "published_at",
        table_alias,
        max_id,
        min_id,
    )
}

fn append_status_id_cursor_parts<'a>(
    bindings: &mut Vec<D1Type<'a>>,
    source_table: &str,
    timestamp_column: &str,
    table_alias: &str,
    max_id: Option<&'a str>,
    min_id: Option<&'a str>,
) -> StatusIdCursorParts {
    let mut parts = StatusIdCursorParts::default();
    let qualified_timestamp = qualified_column(table_alias, timestamp_column);
    let qualified_id = qualified_column(table_alias, "id");

    if let Some(max_id) = max_id {
        bindings.push(D1Type::Text(max_id));
        let slot = bindings.len();
        parts.with_clauses.push(format!(
            "max_cursor AS (
                SELECT id, {timestamp_column}
                FROM {source_table}
                WHERE id = ?{slot}
                LIMIT 1
             )"
        ));
        parts.predicates.push(format!(
            "EXISTS (
                SELECT 1
                FROM max_cursor
                WHERE {qualified_timestamp} < max_cursor.{timestamp_column}
                   OR ({qualified_timestamp} = max_cursor.{timestamp_column} AND {qualified_id} < max_cursor.id)
            )"
        ));
    }

    if let Some(min_id) = min_id {
        bindings.push(D1Type::Text(min_id));
        let slot = bindings.len();
        parts.with_clauses.push(format!(
            "min_cursor AS (
                SELECT id, {timestamp_column}
                FROM {source_table}
                WHERE id = ?{slot}
                LIMIT 1
             )"
        ));
        parts.predicates.push(format!(
            "EXISTS (
                SELECT 1
                FROM min_cursor
                WHERE {qualified_timestamp} > min_cursor.{timestamp_column}
                   OR ({qualified_timestamp} = min_cursor.{timestamp_column} AND {qualified_id} > min_cursor.id)
            )"
        ));
    }

    parts
}

pub(crate) fn format_with_clauses(with_clauses: &[String]) -> String {
    if with_clauses.is_empty() {
        String::new()
    } else {
        format!("WITH {}\n         ", with_clauses.join(",\n         "))
    }
}

fn qualified_column(table_alias: &str, column: &str) -> String {
    if table_alias.is_empty() {
        column.to_owned()
    } else {
        format!("{table_alias}.{column}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cursor() -> ResolvedTimelineCursor {
        ResolvedTimelineCursor::default()
    }

    fn full_cursor() -> ResolvedTimelineCursor {
        ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
            min_id: Some("status-min".to_owned()),
        }
    }

    #[test]
    fn resolved_cursor_bindings_only_append_present_bounds() {
        let mut bindings = vec![D1Type::Text("viewer")];
        let cursor = full_cursor();
        let slots = append_resolved_timeline_cursor_bindings(&mut bindings, &cursor);

        assert_eq!(bindings.len(), 5);
        assert_eq!(slots.max_timestamp, Some(2));
        assert_eq!(slots.max_id, Some(3));
        assert_eq!(slots.min_timestamp, Some(4));
        assert_eq!(slots.min_id, Some(5));
    }

    #[test]
    fn resolved_cursor_bindings_skip_open_bounds() {
        let mut bindings = Vec::new();
        let cursor = empty_cursor();
        let slots = append_resolved_timeline_cursor_bindings(&mut bindings, &cursor);

        assert!(bindings.is_empty());
        assert_eq!(slots, ResolvedTimelineCursorSlots::default());
    }

    #[test]
    fn seekable_resolved_cursor_predicates_emit_index_friendly_comparisons() {
        let mut bindings = Vec::new();
        let cursor = full_cursor();
        let slots = append_resolved_timeline_cursor_bindings(&mut bindings, &cursor);
        let predicates =
            seekable_resolved_timeline_cursor_predicates("s.created_at", "s.id", &slots);

        assert!(predicates.contains("s.created_at <= ?1"));
        assert!(predicates.contains("(s.created_at < ?1 OR s.id < ?2)"));
        assert!(predicates.contains("s.created_at >= ?3"));
        assert!(predicates.contains("(s.created_at > ?3 OR s.id > ?4)"));
        assert!(!predicates.contains("IS NULL"));
    }

    #[test]
    fn min_timestamp_cursor_predicates_support_optional_id_tie_break() {
        let mut with_id = Vec::new();
        let slots_with_id =
            append_min_timestamp_cursor_bindings(&mut with_id, "2026-01-01T00:00:00Z", Some("s-1"));
        let predicates_with_id =
            seekable_min_timestamp_cursor_predicates("s.created_at", "s.id", &slots_with_id);
        assert!(predicates_with_id.contains("s.created_at > ?1"));
        assert!(predicates_with_id.contains("s.id > ?2"));

        let mut without_id = Vec::new();
        let slots_without_id =
            append_min_timestamp_cursor_bindings(&mut without_id, "2026-01-01T00:00:00Z", None);
        let predicates_without_id =
            seekable_min_timestamp_cursor_predicates("s.created_at", "s.id", &slots_without_id);
        assert_eq!(predicates_without_id.trim(), "AND s.created_at > ?1");
    }

    #[test]
    fn status_id_cursor_parts_only_emit_present_bounds() {
        let mut bindings = vec![D1Type::Text("acct-1")];
        let parts = append_local_status_id_cursor_parts(
            &mut bindings,
            "statuses",
            Some("status-max"),
            Some("status-min"),
        );

        assert_eq!(bindings.len(), 3);
        assert_eq!(parts.with_clauses.len(), 2);
        assert_eq!(parts.predicates.len(), 2);
        assert!(!parts.predicates.join(" ").contains("IS NULL"));
    }
}
