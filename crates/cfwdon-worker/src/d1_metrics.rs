use std::cell::{Cell, RefCell};

use worker::{Response, Result};

/// Per-query duration that triggers a structured `d1_slow_query` log line.
pub(crate) const D1_SLOW_QUERY_MS: u64 = 500;
/// Previous-request D1 SQL budget that triggers 503 load-shedding on heavy public reads.
pub(crate) const D1_LOAD_SHED_SQL_MS: u64 = 8_000;
const D1_LOAD_SHED_RETRY_AFTER_SECS: u32 = 5;
const D1_STATEMENT_FAMILY_MAX_LEN: usize = 160;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct D1RequestMetrics {
    pub query_count: u32,
    pub sql_ms_sum: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct D1QueryIdentity {
    pub query_name: String,
    pub statement_family: String,
    pub operation: &'static str,
}

impl D1QueryIdentity {
    pub(crate) fn from_sql(sql: &str, operation: &'static str) -> Self {
        let statement_family = d1_statement_family(sql);
        let query_name = d1_query_name_from_family(&statement_family);
        Self {
            query_name,
            statement_family,
            operation,
        }
    }
}

thread_local! {
    static D1_REQUEST_METRICS: RefCell<D1RequestMetrics> =
        RefCell::new(D1RequestMetrics::default());
    static LAST_REQUEST_D1_SQL_MS: Cell<u64> = const { Cell::new(0) };
    static D1_REQUEST_ROUTE: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(crate) fn reset_d1_request_metrics() {
    D1_REQUEST_METRICS.with(|metrics| *metrics.borrow_mut() = D1RequestMetrics::default());
    D1_REQUEST_ROUTE.with(|route| *route.borrow_mut() = None);
}

pub(crate) fn bind_d1_request_route(route: impl Into<String>) {
    D1_REQUEST_ROUTE.with(|slot| *slot.borrow_mut() = Some(route.into()));
}

pub(crate) fn d1_request_route(method: &str, path: &str) -> String {
    format!("{method} {}", sanitize_d1_route_path(path))
}

pub(crate) fn snapshot_d1_request_metrics() -> D1RequestMetrics {
    D1_REQUEST_METRICS.with(|metrics| *metrics.borrow())
}

#[cfg(test)]
pub(crate) fn record_d1_query_duration(duration_ms: u64) {
    record_d1_query(duration_ms, None);
}

pub(crate) fn record_d1_query(duration_ms: u64, identity: Option<&D1QueryIdentity>) {
    D1_REQUEST_METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        metrics.query_count = metrics.query_count.saturating_add(1);
        metrics.sql_ms_sum = metrics.sql_ms_sum.saturating_add(duration_ms);
    });
    maybe_log_slow_d1_query(duration_ms, identity);
}

pub(crate) fn record_d1_wall_clock(started_at_ms: f64, identity: &D1QueryIdentity) {
    let duration_ms = (js_sys::Date::now() - started_at_ms).max(0.0).round() as u64;
    record_d1_query(duration_ms, Some(identity));
}

fn maybe_log_slow_d1_query(duration_ms: u64, identity: Option<&D1QueryIdentity>) {
    if duration_ms < D1_SLOW_QUERY_MS {
        return;
    }
    let payload = d1_slow_query_payload(duration_ms, identity);
    let message = d1_slow_query_message(duration_ms, identity);
    // console logging is wasm-only; native unit tests still build the payload.
    #[cfg(target_arch = "wasm32")]
    crate::log_json_event(crate::add_log_message(payload, message));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (payload, message);
}

fn d1_slow_query_payload(
    duration_ms: u64,
    identity: Option<&D1QueryIdentity>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "event": "d1_slow_query",
        "component": "d1",
        "duration_ms": duration_ms,
        "threshold_ms": D1_SLOW_QUERY_MS,
    });
    if let (Some(object), Some(identity)) = (payload.as_object_mut(), identity) {
        object.insert(
            "query_name".to_owned(),
            serde_json::json!(identity.query_name),
        );
        object.insert(
            "statement_family".to_owned(),
            serde_json::json!(identity.statement_family),
        );
        object.insert(
            "operation".to_owned(),
            serde_json::json!(identity.operation),
        );
    }
    if let (Some(object), Some(route)) = (payload.as_object_mut(), current_d1_request_route()) {
        object.insert("route".to_owned(), serde_json::json!(route));
    }
    payload
}

fn d1_slow_query_message(duration_ms: u64, identity: Option<&D1QueryIdentity>) -> String {
    match identity {
        Some(identity) => format!(
            "D1 query {} exceeded {D1_SLOW_QUERY_MS}ms threshold ({duration_ms}ms)",
            identity.query_name
        ),
        None => format!("D1 query exceeded {D1_SLOW_QUERY_MS}ms threshold ({duration_ms}ms)"),
    }
}

fn current_d1_request_route() -> Option<String> {
    D1_REQUEST_ROUTE.with(|slot| slot.borrow().clone())
}

pub(crate) fn d1_statement_family(sql: &str) -> String {
    let normalized = collapse_sql_whitespace_lower(&replace_sql_string_literals(sql));
    let after_with = sql_after_with_clause(&normalized);
    truncate_statement_family(after_with)
}

fn collapse_sql_whitespace_lower(sql: &str) -> String {
    let mut out = String::new();
    let mut prev_space = true;
    for ch in sql.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
            continue;
        }
        out.push(ch.to_ascii_lowercase());
        prev_space = false;
    }
    out.trim().to_owned()
}

fn truncate_statement_family(normalized: &str) -> String {
    if normalized.len() <= D1_STATEMENT_FAMILY_MAX_LEN {
        return normalized.to_owned();
    }
    normalized
        .char_indices()
        .take_while(|(index, _)| *index < D1_STATEMENT_FAMILY_MAX_LEN)
        .last()
        .map(|(index, ch)| normalized[..index + ch.len_utf8()].trim().to_owned())
        .unwrap_or_default()
}

/// Drop a leading `WITH [RECURSIVE] cte AS (...) [, ...]` list so query names
/// use the outer statement's verb and table instead of `with:unknown`.
fn sql_after_with_clause(normalized: &str) -> &str {
    let Some(rest) = normalized.strip_prefix("with ") else {
        return normalized;
    };
    let rest = rest.strip_prefix("recursive ").unwrap_or(rest);
    let mut index = 0;
    loop {
        index = skip_sql_whitespace(rest, index);
        let ident_start = index;
        index = skip_sql_ident(rest, index);
        if index == ident_start {
            return normalized;
        }
        index = skip_sql_whitespace(rest, index);
        if rest.as_bytes().get(index) == Some(&b'(') {
            let Some(after_cols) = skip_balanced_parens(rest, index) else {
                return normalized;
            };
            index = after_cols;
            index = skip_sql_whitespace(rest, index);
        }
        if !rest[index..].starts_with("as") {
            return normalized;
        }
        index += 2;
        index = skip_sql_whitespace(rest, index);
        if rest.as_bytes().get(index) != Some(&b'(') {
            return normalized;
        }
        let Some(after_body) = skip_balanced_parens(rest, index) else {
            return normalized;
        };
        index = skip_sql_whitespace(rest, after_body);
        if rest.as_bytes().get(index) == Some(&b',') {
            index += 1;
            continue;
        }
        let remainder = rest[index..].trim();
        return if remainder.is_empty() {
            normalized
        } else {
            remainder
        };
    }
}

fn skip_sql_whitespace(sql: &str, mut index: usize) -> usize {
    let bytes = sql.as_bytes();
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn skip_sql_ident(sql: &str, start: usize) -> usize {
    let bytes = sql.as_bytes();
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            index += 1;
            continue;
        }
        break;
    }
    index
}

fn skip_balanced_parens(sql: &str, open_at: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    if bytes.get(open_at) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    for (offset, byte) in bytes[open_at..].iter().enumerate() {
        match byte {
            b'(' => depth = depth.saturating_add(1),
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open_at + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn d1_query_name_from_family(statement_family: &str) -> String {
    let verb = first_sql_verb(statement_family);
    let table = first_sql_table(statement_family).unwrap_or("unknown");
    format!("{verb}:{table}")
}

fn first_sql_verb(normalized: &str) -> &'static str {
    match normalized.split_whitespace().next().unwrap_or("") {
        "select" => "select",
        "insert" => "insert",
        "update" => "update",
        "delete" => "delete",
        "replace" => "replace",
        "with" => "with",
        _ => "query",
    }
}

fn first_sql_table(normalized: &str) -> Option<&str> {
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    for index in 0..tokens.len() {
        let Some(table) = tokens.get(index + 1) else {
            continue;
        };
        let matched = matches!(tokens[index], "from" | "into" | "update" | "join");
        if !matched {
            continue;
        }
        let table = table.trim_matches(|ch| matches!(ch, '(' | ')' | '`' | '"' | '[' | ']'));
        if table.is_empty() || table == "select" || table == "values" || table.starts_with('(') {
            continue;
        }
        return Some(table);
    }
    None
}

fn replace_sql_string_literals(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\'' {
            out.push(ch);
            continue;
        }
        out.push('?');
        while let Some(inner) = chars.next() {
            if inner != '\'' {
                continue;
            }
            if chars.peek() == Some(&'\'') {
                chars.next();
                continue;
            }
            break;
        }
    }
    out
}

fn sanitize_d1_route_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for segment in path.split('/') {
        if segment.is_empty() {
            continue;
        }
        out.push('/');
        if looks_like_route_id(segment) {
            out.push_str(":id");
        } else {
            out.push_str(segment);
        }
    }
    if out.is_empty() { "/".to_owned() } else { out }
}

fn looks_like_route_id(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    if segment.chars().all(|ch| ch.is_ascii_digit()) {
        return true;
    }
    let hexish = segment
        .chars()
        .filter(|ch| *ch != '-')
        .all(|ch| ch.is_ascii_hexdigit());
    hexish && segment.len() >= 16
}

/// Remember this request's D1 SQL cost so the next request in the same isolate
/// can fail-fast under sustained D1 pressure instead of hanging until cancellation.
pub(crate) fn publish_d1_request_pressure() {
    let metrics = snapshot_d1_request_metrics();
    LAST_REQUEST_D1_SQL_MS.with(|slot| slot.set(metrics.sql_ms_sum));
}

pub(crate) fn last_request_d1_sql_ms() -> u64 {
    LAST_REQUEST_D1_SQL_MS.with(|slot| slot.get())
}

pub(crate) fn d1_pressure_load_shed_response() -> Result<Option<Response>> {
    let previous_sql_ms = last_request_d1_sql_ms();
    if previous_sql_ms < D1_LOAD_SHED_SQL_MS {
        return Ok(None);
    }

    let mut response = Response::from_json(&serde_json::json!({
        "error": "Service temporarily unavailable",
        "error_description": "D1 latency pressure; retry shortly",
    }))?
    .with_status(503);
    response
        .headers_mut()
        .set("Retry-After", &D1_LOAD_SHED_RETRY_AFTER_SECS.to_string())?;
    Ok(Some(response))
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

    #[test]
    fn publish_d1_request_pressure_tracks_previous_request() {
        reset_d1_request_metrics();
        LAST_REQUEST_D1_SQL_MS.with(|slot| slot.set(0));
        record_d1_query_duration(D1_LOAD_SHED_SQL_MS);
        publish_d1_request_pressure();
        assert_eq!(last_request_d1_sql_ms(), D1_LOAD_SHED_SQL_MS);
        reset_d1_request_metrics();
        LAST_REQUEST_D1_SQL_MS.with(|slot| slot.set(0));
    }

    #[test]
    fn d1_query_identity_names_instance_and_timeline_sql() {
        assert_eq!(
            D1QueryIdentity::from_sql(
                "SELECT domain, title, description
                 FROM instance_settings
                 WHERE id = 1
                 LIMIT 1",
                "first"
            )
            .query_name,
            "select:instance_settings"
        );
        assert_eq!(
            D1QueryIdentity::from_sql(
                "SELECT COUNT(DISTINCT account_id) AS count
                 FROM statuses
                 WHERE created_at >= datetime('now', '-28 days')",
                "first"
            )
            .query_name,
            "select:statuses"
        );
        assert_eq!(
            D1QueryIdentity::from_sql("INSERT INTO accounts (id) VALUES (?1)", "run").query_name,
            "insert:accounts"
        );
        assert_eq!(
            D1QueryIdentity::from_sql("UPDATE accounts SET username = ?1 WHERE id = ?2", "run")
                .query_name,
            "update:accounts"
        );
        assert_eq!(
            D1QueryIdentity::from_sql("DELETE FROM follow_requests WHERE id = ?1", "run")
                .query_name,
            "delete:follow_requests"
        );
        assert_eq!(
            D1QueryIdentity::from_sql(
                "WITH requested(id) AS (VALUES (?1), (?2))
                 UPDATE media_attachments
                 SET status_id = ?3
                 WHERE id IN (SELECT id FROM requested)",
                "run"
            )
            .query_name,
            "update:media_attachments"
        );
        assert_eq!(
            D1QueryIdentity::from_sql(
                "WITH max_cursor AS (
                    SELECT id, created_at
                    FROM remote_statuses
                    WHERE id = ?1
                    LIMIT 1
                 )
                 SELECT rs.id
                 FROM remote_statuses rs
                 WHERE EXISTS (
                    SELECT 1 FROM max_cursor
                    WHERE rs.published_at < max_cursor.created_at
                 )",
                "all"
            )
            .query_name,
            "select:remote_statuses"
        );
        assert_eq!(
            D1QueryIdentity::from_sql(
                "SELECT id FROM (SELECT id FROM accounts WHERE username = ?1)",
                "first"
            )
            .query_name,
            "select:accounts"
        );
    }

    #[test]
    fn d1_statement_family_strips_literals_and_collapses_whitespace() {
        assert_eq!(
            d1_statement_family("SELECT * FROM accounts WHERE handle = 'alice'"),
            "select * from accounts where handle = ?"
        );
        assert_eq!(
            d1_statement_family("SELECT * FROM accounts WHERE handle = 'O''Brien'"),
            "select * from accounts where handle = ?"
        );
        assert_eq!(
            d1_statement_family(
                "WITH requested(id) AS (VALUES (?1))
                 UPDATE media_attachments SET status_id = ?2"
            ),
            "update media_attachments set status_id = ?2"
        );
        assert_eq!(
            d1_statement_family(
                "WITH max_cursor AS (SELECT id FROM statuses WHERE id = ?1 LIMIT 1),
                      min_cursor AS (SELECT id FROM statuses WHERE id = ?2 LIMIT 1)
                 SELECT id FROM statuses"
            ),
            "select id from statuses"
        );
    }

    #[test]
    fn d1_slow_query_payload_includes_identity_and_route() {
        reset_d1_request_metrics();
        bind_d1_request_route(d1_request_route("GET", "/api/v1/instance"));
        let identity = D1QueryIdentity::from_sql(
            "SELECT COUNT(DISTINCT account_id) AS count FROM statuses",
            "first",
        );
        let payload = d1_slow_query_payload(854, Some(&identity));
        assert_eq!(payload["event"], "d1_slow_query");
        assert_eq!(payload["query_name"], "select:statuses");
        assert_eq!(payload["operation"], "first");
        assert_eq!(payload["route"], "GET /api/v1/instance");
        assert_eq!(payload["duration_ms"], 854);
        assert_eq!(
            d1_slow_query_message(854, Some(&identity)),
            "D1 query select:statuses exceeded 500ms threshold (854ms)"
        );
        reset_d1_request_metrics();
    }

    #[test]
    fn d1_request_route_replaces_id_segments() {
        assert_eq!(
            d1_request_route("GET", "/api/v1/statuses/018f1a2b3c4d5e6f7a8b9c0d"),
            "GET /api/v1/statuses/:id"
        );
        assert_eq!(
            d1_request_route("GET", "/api/v1/timelines/tag/rust"),
            "GET /api/v1/timelines/tag/rust"
        );
    }
}
