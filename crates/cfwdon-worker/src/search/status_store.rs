use std::collections::HashSet;

use crate::{
    RemoteActorRow, RemoteStatusRecord, RemoteStatusRow, Result, StatusRecord, StatusRow,
    remote_status_from_record, status_from_record,
};
use worker::D1Database;
use worker::d1::D1Type;

const LOCAL_STATUS_SEARCH_SELECT: &str = "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses";

const REMOTE_STATUS_SEARCH_SELECT: &str = "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.boost_of_uri,
                rs.quote_of_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.quote_state,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url,
                ra.locked,
                ra.bot,
                ra.discoverable,
                ra.indexable
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri";

const LOCAL_STATUS_SEARCH_COLUMNS: &[&str] = &["text_content", "spoiler_text"];
const REMOTE_STATUS_SEARCH_COLUMNS: &[&str] = &["content_html", "spoiler_text"];

fn normalized_search_patterns(queries: &[String]) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut seen = HashSet::new();
    for query in queries {
        let normalized = query.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        let pattern = format!("%{}%", normalized);
        if seen.insert(pattern.clone()) {
            patterns.push(pattern);
        }
    }
    if patterns.is_empty() {
        patterns.push("%".to_owned());
    }
    patterns
}

fn search_like_clauses(columns: &[&str], start_index: usize, pattern_count: usize) -> String {
    // Each search term gets one numbered binding that is reused across all searched columns.
    (0..pattern_count)
        .map(|pattern_offset| {
            let binding = start_index + pattern_offset;
            let column_clause = columns
                .iter()
                .map(|column| format!("lower({column}) LIKE ?{binding}"))
                .collect::<Vec<_>>()
                .join(" OR ");
            format!("({column_clause})")
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn search_patterns_match_everything(patterns: &[String]) -> bool {
    matches!(patterns, [pattern] if pattern == "%")
}

fn optional_text_binding(value: Option<&str>) -> D1Type<'_> {
    match value {
        Some(value) => D1Type::Text(value),
        None => D1Type::Null,
    }
}

fn push_search_pattern_bindings<'a>(bindings: &mut Vec<D1Type<'a>>, patterns: &'a [String]) {
    bindings.extend(
        patterns
            .iter()
            .map(|pattern| D1Type::Text(pattern.as_str())),
    );
}

fn push_cursor_bindings<'a>(
    bindings: &mut Vec<D1Type<'a>>,
    max_timestamp: Option<&'a str>,
    max_id: Option<&'a str>,
    min_timestamp: Option<&'a str>,
    min_id: Option<&'a str>,
    limit: u32,
) {
    bindings.push(optional_text_binding(max_timestamp));
    bindings.push(optional_text_binding(max_id));
    bindings.push(optional_text_binding(min_timestamp));
    bindings.push(optional_text_binding(min_id));
    bindings.push(D1Type::Integer(limit as i32));
}

fn cursor_window_clause(
    timestamp_column: &str,
    id_column: &str,
    max_ts: usize,
    max_id: usize,
    min_ts: usize,
    min_id: usize,
) -> String {
    format!(
        "AND (?{max_ts} IS NULL
                    OR {timestamp_column} < ?{max_ts}
                    OR ({timestamp_column} = ?{max_ts} AND {id_column} < ?{max_id}))
               AND (?{min_ts} IS NULL
                    OR {timestamp_column} > ?{min_ts}
                    OR ({timestamp_column} = ?{min_ts} AND {id_column} > ?{min_id}))"
    )
}

fn cursor_window_clause_after_patterns(
    timestamp_column: &str,
    id_column: &str,
    pattern_max_index: usize,
) -> String {
    cursor_window_clause(
        timestamp_column,
        id_column,
        pattern_max_index + 1,
        pattern_max_index + 2,
        pattern_max_index + 3,
        pattern_max_index + 4,
    )
}

fn limit_binding_index(pattern_max_index: usize) -> usize {
    pattern_max_index + 5
}

fn pattern_max_binding_index(pattern_count: usize, leading_filter_count: usize) -> usize {
    leading_filter_count + pattern_count
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .unwrap_or_default()
        .to_owned()
}

fn optional_json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(ToOwned::to_owned)
}

fn remote_status_row_from_search_value(value: &serde_json::Value) -> RemoteStatusRow {
    remote_status_from_record(RemoteStatusRecord {
        id: json_string(value, "id"),
        actor_uri: json_string(value, "actor_uri"),
        object_uri: json_string(value, "object_uri"),
        url: optional_json_string(value, "url"),
        in_reply_to_uri: optional_json_string(value, "in_reply_to_uri"),
        boost_of_uri: optional_json_string(value, "boost_of_uri"),
        quote_of_uri: optional_json_string(value, "quote_of_uri"),
        content_html: json_string(value, "content_html"),
        spoiler_text: json_string(value, "spoiler_text"),
        visibility: json_string(value, "visibility"),
        sensitive: value
            .get("sensitive")
            .and_then(|field| field.as_i64())
            .unwrap_or_default() as i32,
        language: optional_json_string(value, "language"),
        quote_state: value
            .get("quote_state")
            .and_then(|field| field.as_str())
            .unwrap_or("accepted")
            .to_owned(),
        published_at: json_string(value, "published_at"),
    })
}

fn remote_search_rows_from_values(
    values: Vec<serde_json::Value>,
) -> Vec<(RemoteStatusRow, RemoteActorRow)> {
    values
        .into_iter()
        .map(|value| {
            (
                remote_status_row_from_search_value(&value),
                RemoteActorRow::from_value(&value),
            )
        })
        .collect()
}

pub(crate) async fn search_local_status_rows(
    db: &D1Database,
    queries: &[String],
    limit: u32,
    account_id: Option<&str>,
    max_id: Option<&str>,
    max_timestamp: Option<&str>,
    min_id: Option<&str>,
    min_timestamp: Option<&str>,
) -> Result<Vec<StatusRow>> {
    let patterns = normalized_search_patterns(queries);
    let pattern_count = if search_patterns_match_everything(&patterns) {
        0
    } else {
        patterns.len()
    };
    let search_clauses = search_like_clauses(LOCAL_STATUS_SEARCH_COLUMNS, 2, pattern_count);
    let search_filter = if pattern_count == 0 {
        "1 = 1".to_owned()
    } else {
        format!("({search_clauses})")
    };
    let result = if let Some(account_id) = account_id {
        let mut bindings = Vec::with_capacity(2 + pattern_count + 5);
        bindings.push(D1Type::Text(account_id));
        push_search_pattern_bindings(&mut bindings, &patterns[..pattern_count]);
        push_cursor_bindings(
            &mut bindings,
            max_timestamp,
            max_id,
            min_timestamp,
            min_id,
            limit,
        );
        let pattern_max_index = pattern_max_binding_index(pattern_count, 1);
        let cursor_clause =
            cursor_window_clause_after_patterns("created_at", "id", pattern_max_index);
        db.prepare(format!(
            "{LOCAL_STATUS_SEARCH_SELECT}
             WHERE account_id = ?1
               AND {search_filter}
               {cursor_clause}
             ORDER BY created_at DESC, id DESC
             LIMIT ?{limit}",
            limit = limit_binding_index(pattern_max_index),
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let mut bindings = Vec::with_capacity(pattern_count + 5);
        push_search_pattern_bindings(&mut bindings, &patterns[..pattern_count]);
        push_cursor_bindings(
            &mut bindings,
            max_timestamp,
            max_id,
            min_timestamp,
            min_id,
            limit,
        );
        let pattern_max_index = pattern_max_binding_index(pattern_count, 0);
        let cursor_clause =
            cursor_window_clause_after_patterns("created_at", "id", pattern_max_index);
        db.prepare(format!(
            "{LOCAL_STATUS_SEARCH_SELECT}
             WHERE {search_filter}
               {cursor_clause}
             ORDER BY created_at DESC, id DESC
             LIMIT ?{limit}",
            limit = limit_binding_index(pattern_max_index),
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    result
        .results::<StatusRecord>()
        .map(|rows| rows.into_iter().map(status_from_record).collect())
}

pub(crate) async fn search_remote_status_rows(
    db: &D1Database,
    queries: &[String],
    limit: u32,
    actor_uri: Option<&str>,
    max_id: Option<&str>,
    max_timestamp: Option<&str>,
    min_id: Option<&str>,
    min_timestamp: Option<&str>,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let patterns = normalized_search_patterns(queries);
    let pattern_count = if search_patterns_match_everything(&patterns) {
        0
    } else {
        patterns.len()
    };
    let search_clauses = search_like_clauses(REMOTE_STATUS_SEARCH_COLUMNS, 2, pattern_count);
    let search_filter = if pattern_count == 0 {
        "1 = 1".to_owned()
    } else {
        format!("({search_clauses})")
    };
    let result = if let Some(actor_uri) = actor_uri {
        let mut bindings = Vec::with_capacity(2 + pattern_count + 5);
        bindings.push(D1Type::Text(actor_uri));
        push_search_pattern_bindings(&mut bindings, &patterns[..pattern_count]);
        push_cursor_bindings(
            &mut bindings,
            max_timestamp,
            max_id,
            min_timestamp,
            min_id,
            limit,
        );
        let pattern_max_index = pattern_max_binding_index(pattern_count, 1);
        let cursor_clause =
            cursor_window_clause_after_patterns("rs.published_at", "rs.id", pattern_max_index);
        db.prepare(format!(
            "{REMOTE_STATUS_SEARCH_SELECT}
             WHERE rs.actor_uri = ?1
               AND {search_filter}
               {cursor_clause}
             ORDER BY rs.published_at DESC, rs.id DESC
             LIMIT ?{limit}",
            limit = limit_binding_index(pattern_max_index),
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let mut bindings = Vec::with_capacity(pattern_count + 5);
        push_search_pattern_bindings(&mut bindings, &patterns[..pattern_count]);
        push_cursor_bindings(
            &mut bindings,
            max_timestamp,
            max_id,
            min_timestamp,
            min_id,
            limit,
        );
        let pattern_max_index = pattern_max_binding_index(pattern_count, 0);
        let cursor_clause =
            cursor_window_clause_after_patterns("rs.published_at", "rs.id", pattern_max_index);
        db.prepare(format!(
            "{REMOTE_STATUS_SEARCH_SELECT}
             WHERE {search_filter}
               {cursor_clause}
             ORDER BY rs.published_at DESC, rs.id DESC
             LIMIT ?{limit}",
            limit = limit_binding_index(pattern_max_index),
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    Ok(remote_search_rows_from_values(
        result.results::<serde_json::Value>()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_search_patterns_trims_lowercases_and_deduplicates() {
        assert_eq!(
            normalized_search_patterns(&[
                " Rust ".to_owned(),
                "rust".to_owned(),
                "RUST".to_owned(),
                "fediverse".to_owned(),
            ]),
            vec!["%rust%".to_owned(), "%fediverse%".to_owned()]
        );
    }

    #[test]
    fn normalized_search_patterns_defaults_to_wildcard() {
        assert_eq!(
            normalized_search_patterns(&[" ".to_owned(), "\t".to_owned()]),
            vec!["%".to_owned()]
        );
        assert!(search_patterns_match_everything(&["%".to_owned()]));
    }

    #[test]
    fn search_like_clauses_uses_expected_binding_offsets() {
        assert_eq!(
            search_like_clauses(&["content_html", "spoiler_text"], 2, 2),
            "(lower(content_html) LIKE ?2 OR lower(spoiler_text) LIKE ?2) OR (lower(content_html) LIKE ?3 OR lower(spoiler_text) LIKE ?3)"
        );
    }

    #[test]
    fn cursor_window_clause_uses_supplied_columns_and_bindings() {
        assert_eq!(
            cursor_window_clause("rs.published_at", "rs.id", 4, 5, 6, 7),
            "AND (?4 IS NULL\n                    OR rs.published_at < ?4\n                    OR (rs.published_at = ?4 AND rs.id < ?5))\n               AND (?6 IS NULL\n                    OR rs.published_at > ?6\n                    OR (rs.published_at = ?6 AND rs.id > ?7))"
        );
    }

    #[test]
    fn cursor_window_clause_after_patterns_offsets_cursor_bindings() {
        assert_eq!(
            cursor_window_clause_after_patterns("created_at", "id", 3),
            cursor_window_clause("created_at", "id", 4, 5, 6, 7)
        );
    }

    #[test]
    fn limit_binding_index_follows_cursor_bindings() {
        assert_eq!(limit_binding_index(3), 8);
    }

    #[test]
    fn pattern_max_binding_index_accounts_for_leading_filters() {
        assert_eq!(pattern_max_binding_index(2, 0), 2);
        assert_eq!(pattern_max_binding_index(2, 1), 3);
    }

    #[test]
    fn remote_status_row_from_search_value_defaults_missing_quote_state() {
        let value = serde_json::json!({
            "id": "rs1",
            "actor_uri": "https://remote.example/users/alice",
            "object_uri": "https://remote.example/statuses/1",
            "content_html": "<p>hello</p>",
            "spoiler_text": "",
            "visibility": "public",
            "sensitive": 0,
            "published_at": "2025-01-01T00:00:00Z"
        });

        let row = remote_status_row_from_search_value(&value);

        assert_eq!(row.id, "rs1");
        assert_eq!(row.quote_state, cfwdon_domain::QuoteState::Accepted);
        assert_eq!(row.url, None);
        assert_eq!(row.in_reply_to_uri, None);
        assert_eq!(row.boost_of_uri, None);
        assert_eq!(row.quote_of_uri, None);
    }

    #[test]
    fn remote_search_rows_from_values_preserves_order() {
        let first = serde_json::json!({
            "id": "rs1",
            "actor_uri": "https://remote.example/users/alice",
            "object_uri": "https://remote.example/statuses/1",
            "content_html": "<p>one</p>",
            "spoiler_text": "",
            "visibility": "public",
            "sensitive": 0,
            "published_at": "2025-01-01T00:00:00Z",
            "username": "alice",
            "domain": "remote.example"
        });
        let second = serde_json::json!({
            "id": "rs2",
            "actor_uri": "https://remote.example/users/bob",
            "object_uri": "https://remote.example/statuses/2",
            "content_html": "<p>two</p>",
            "spoiler_text": "",
            "visibility": "public",
            "sensitive": 0,
            "published_at": "2025-01-02T00:00:00Z",
            "username": "bob",
            "domain": "remote.example"
        });

        let rows = remote_search_rows_from_values(vec![first, second]);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0.id, "rs1");
        assert_eq!(rows[0].1.username, "alice");
        assert_eq!(rows[1].0.id, "rs2");
        assert_eq!(rows[1].1.username, "bob");
    }
}
