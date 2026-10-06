use super::{NotificationsQuery, is_legacy_notification_api_id};
use crate::time_html::timestamp_to_mastodon_iso8601;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use worker::d1::D1Type;

/// How a notification source table stores its timestamps. Rows written with
/// SQLite's `CURRENT_TIMESTAMP` read `YYYY-MM-DD HH:MM:SS`; rows written from
/// Rust or ActivityPub read `YYYY-MM-DDTHH:MM:SS…Z`. The two never compare
/// correctly as text (`'…T01:00' > '… 09:00'`), so a bound has to be spelled in
/// the format of the column it is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredTimestampFormat {
    Sqlite,
    Iso,
}

/// The creation-time range a notification listing can be limited to, in whole
/// Unix seconds (both ends inclusive). Snowflake API ids carry their creation
/// time, so cursors become SQL bounds; the exact id comparison still happens
/// in Rust, which keeps ties within one second correct.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NotificationTimeWindow {
    since: Option<i64>,
    until: Option<i64>,
}

/// SQL bounds for one column: `column >= lower AND column < upper`.
#[derive(Debug, Default)]
pub(crate) struct SqlTimeBounds {
    lower: Option<String>,
    upper: Option<String>,
}

/// Creation time, in whole seconds, of a snowflake notification id. Legacy
/// hash ids and internal keys carry no time.
pub(crate) fn notification_cursor_unix_seconds(cursor: Option<&str>) -> Option<i64> {
    let id = cursor?.trim().parse::<i64>().ok()?;
    if id <= 0 || is_legacy_notification_api_id(id) {
        return None;
    }
    Some((id >> 16).div_euclid(1000))
}

fn timestamp_unix_seconds(value: &str) -> Option<i64> {
    OffsetDateTime::parse(&timestamp_to_mastodon_iso8601(value), &Rfc3339)
        .ok()
        .map(OffsetDateTime::unix_timestamp)
}

pub(crate) fn notification_time_window(query: &NotificationsQuery) -> NotificationTimeWindow {
    let since = [
        notification_cursor_unix_seconds(query.min_id.as_deref()),
        notification_cursor_unix_seconds(query.since_id.as_deref()),
        query
            .min_created_at
            .as_deref()
            .and_then(timestamp_unix_seconds),
    ]
    .into_iter()
    .flatten()
    .max();
    NotificationTimeWindow {
        since,
        until: notification_cursor_unix_seconds(query.max_id.as_deref()),
    }
}

/// A cursor that is present but carries no time cannot bound the SQL, so the
/// listing falls back to scanning a wide window and filtering in Rust.
pub(crate) fn notification_query_has_untimed_cursor(query: &NotificationsQuery) -> bool {
    [
        query.max_id.as_deref(),
        query.since_id.as_deref(),
        query.min_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|cursor| !cursor.is_empty())
    .any(|cursor| notification_cursor_unix_seconds(Some(cursor)).is_none())
}

fn format_unix_seconds(seconds: i64, format: StoredTimestampFormat) -> Option<String> {
    let value = OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    let separator = match format {
        StoredTimestampFormat::Sqlite => ' ',
        StoredTimestampFormat::Iso => 'T',
    };
    Some(format!(
        "{:04}-{:02}-{:02}{separator}{:02}:{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
    ))
}

impl NotificationTimeWindow {
    /// Bounds are second-precision prefixes: every spelling of an instant in
    /// second `s` (`…:SS`, `…:SSZ`, `…:SS.fffZ`) sorts at or after the prefix
    /// for `s` and before the prefix for `s + 1`.
    pub(crate) fn sql_bounds(&self, format: StoredTimestampFormat) -> SqlTimeBounds {
        SqlTimeBounds {
            lower: self
                .since
                .and_then(|seconds| format_unix_seconds(seconds, format)),
            upper: self
                .until
                .and_then(|seconds| format_unix_seconds(seconds.saturating_add(1), format)),
        }
    }
}

impl SqlTimeBounds {
    /// `AND` conditions on `column`, numbering placeholders from `first_param`
    /// in the order `bindings` yields them.
    pub(crate) fn clause(&self, column: &str, first_param: usize) -> String {
        let mut clause = String::new();
        let mut param = first_param;
        if self.lower.is_some() {
            clause.push_str(&format!(" AND {column} >= ?{param}"));
            param += 1;
        }
        if self.upper.is_some() {
            clause.push_str(&format!(" AND {column} < ?{param}"));
        }
        clause
    }

    pub(crate) fn bindings(&self) -> impl Iterator<Item = D1Type<'_>> {
        self.lower
            .iter()
            .chain(self.upper.iter())
            .map(|value| D1Type::Text(value.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NotificationTimeWindow, StoredTimestampFormat, notification_query_has_untimed_cursor,
        notification_time_window,
    };
    use crate::notifications::{NotificationsQuery, notification_api_id};

    fn snowflake(created_at: &str) -> String {
        notification_api_id("favourite-local-1-2", created_at).to_string()
    }

    #[test]
    fn cursors_become_second_bounds_in_the_column_format() {
        let query = NotificationsQuery {
            max_id: Some(snowflake("2026-10-07T05:00:00.250Z")),
            since_id: Some(snowflake("2026-10-06 23:59:59")),
            ..NotificationsQuery::default()
        };
        let window = notification_time_window(&query);

        let sqlite = window.sql_bounds(StoredTimestampFormat::Sqlite);
        assert_eq!(sqlite.lower.as_deref(), Some("2026-10-06 23:59:59"));
        assert_eq!(sqlite.upper.as_deref(), Some("2026-10-07 05:00:01"));
        assert_eq!(
            sqlite.clause("f.created_at", 3),
            " AND f.created_at >= ?3 AND f.created_at < ?4"
        );

        let iso = window.sql_bounds(StoredTimestampFormat::Iso);
        assert_eq!(iso.lower.as_deref(), Some("2026-10-06T23:59:59"));
        assert_eq!(iso.upper.as_deref(), Some("2026-10-07T05:00:01"));
        assert!(!notification_query_has_untimed_cursor(&query));
    }

    #[test]
    fn iso_bounds_admit_every_spelling_of_the_boundary_seconds() {
        let window = NotificationTimeWindow {
            since: Some(1_791_349_200), // 2026-10-07T05:00:00Z
            until: Some(1_791_349_200),
        };
        let bounds = window.sql_bounds(StoredTimestampFormat::Iso);
        let (lower, upper) = (bounds.lower.unwrap(), bounds.upper.unwrap());
        for inside in [
            "2026-10-07T05:00:00Z",
            "2026-10-07T05:00:00.000Z",
            "2026-10-07T05:00:00.999Z",
        ] {
            assert!(
                inside >= lower.as_str() && inside < upper.as_str(),
                "{inside}"
            );
        }
        for outside in ["2026-10-07T04:59:59.999Z", "2026-10-07T05:00:01Z"] {
            assert!(
                !(outside >= lower.as_str() && outside < upper.as_str()),
                "{outside}"
            );
        }
    }

    #[test]
    fn min_created_at_bounds_sqlite_rows_from_the_same_day() {
        // The streaming poll passes the last Mastodon-format created_at; a
        // later `CURRENT_TIMESTAMP` row from the same day must stay inside.
        let query = NotificationsQuery {
            min_created_at: Some("2026-10-07T04:00:00.000Z".to_owned()),
            ..NotificationsQuery::default()
        };
        let bounds = notification_time_window(&query).sql_bounds(StoredTimestampFormat::Sqlite);
        let lower = bounds.lower.unwrap();
        assert!("2026-10-07 05:00:00" >= lower.as_str());
        assert!("2026-10-07 03:59:59" < lower.as_str());
        assert_eq!(bounds.upper, None);
    }

    #[test]
    fn legacy_and_internal_cursors_carry_no_time() {
        for cursor in ["favourite-local-1-2", "1000000000000123"] {
            let query = NotificationsQuery {
                max_id: Some(cursor.to_owned()),
                ..NotificationsQuery::default()
            };
            assert_eq!(
                notification_time_window(&query),
                NotificationTimeWindow::default()
            );
            assert!(notification_query_has_untimed_cursor(&query));
        }
    }
}
