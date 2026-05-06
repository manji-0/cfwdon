use crate::{RemoteActorRow, RemoteStatusRow, Result, StatusRow};
use worker::D1Database;
use worker::d1::D1Type;

fn normalized_search_patterns(queries: &[String]) -> Vec<String> {
    let mut patterns = Vec::new();
    for query in queries {
        let normalized = query.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        let pattern = format!("%{}%", normalized);
        if !patterns.iter().any(|value| value == &pattern) {
            patterns.push(pattern);
        }
    }
    if patterns.is_empty() {
        patterns.push("%".to_owned());
    }
    patterns
}

fn search_like_clauses(columns: &[&str], start_index: usize, pattern_count: usize) -> String {
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
    let search_clauses = search_like_clauses(&["text_content", "spoiler_text"], 2, patterns.len());
    let result = if let Some(account_id) = account_id {
        let mut bindings = Vec::with_capacity(2 + patterns.len() + 5);
        bindings.push(D1Type::Text(account_id));
        bindings.extend(
            patterns
                .iter()
                .map(|pattern| D1Type::Text(pattern.as_str())),
        );
        bindings.push(match max_timestamp {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match max_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match min_timestamp {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match min_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(D1Type::Integer(limit as i32));
        let pattern_max_index = 1 + patterns.len();
        db.prepare(
            &format!(
                "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE account_id = ?1
               AND ({})
               AND (?{max_ts} IS NULL
                    OR created_at < ?{max_ts}
                    OR (created_at = ?{max_ts} AND id < ?{max_id}))
               AND (?{min_ts} IS NULL
                    OR created_at > ?{min_ts}
                    OR (created_at = ?{min_ts} AND id > ?{min_id}))
             ORDER BY created_at DESC, id DESC
             LIMIT ?{limit}",
                search_clauses,
                max_ts = pattern_max_index + 1,
                max_id = pattern_max_index + 2,
                min_ts = pattern_max_index + 3,
                min_id = pattern_max_index + 4,
                limit = pattern_max_index + 5,
            ),
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let mut bindings = Vec::with_capacity(patterns.len() + 5);
        bindings.extend(
            patterns
                .iter()
                .map(|pattern| D1Type::Text(pattern.as_str())),
        );
        bindings.push(match max_timestamp {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match max_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match min_timestamp {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match min_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(D1Type::Integer(limit as i32));
        let pattern_max_index = patterns.len();
        db.prepare(
            &format!(
                "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE ({})
               AND (?{max_ts} IS NULL
                    OR created_at < ?{max_ts}
                    OR (created_at = ?{max_ts} AND id < ?{max_id}))
               AND (?{min_ts} IS NULL
                    OR created_at > ?{min_ts}
                    OR (created_at = ?{min_ts} AND id > ?{min_id}))
             ORDER BY created_at DESC, id DESC
             LIMIT ?{limit}",
                search_clauses,
                max_ts = pattern_max_index + 1,
                max_id = pattern_max_index + 2,
                min_ts = pattern_max_index + 3,
                min_id = pattern_max_index + 4,
                limit = pattern_max_index + 5,
            ),
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    result.results::<StatusRow>()
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
    let search_clauses = search_like_clauses(&["content_html", "spoiler_text"], 2, patterns.len());
    let result = if let Some(actor_uri) = actor_uri {
        let mut bindings = Vec::with_capacity(2 + patterns.len() + 5);
        bindings.push(D1Type::Text(actor_uri));
        bindings.extend(
            patterns
                .iter()
                .map(|pattern| D1Type::Text(pattern.as_str())),
        );
        bindings.push(match max_timestamp {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match max_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match min_timestamp {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match min_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(D1Type::Integer(limit as i32));
        let pattern_max_index = 1 + patterns.len();
        db.prepare(&format!(
            "SELECT
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
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE rs.actor_uri = ?1
               AND ({})
               AND (?{max_ts} IS NULL
                    OR rs.published_at < ?{max_ts}
                    OR (rs.published_at = ?{max_ts} AND rs.id < ?{max_id}))
               AND (?{min_ts} IS NULL
                    OR rs.published_at > ?{min_ts}
                    OR (rs.published_at = ?{min_ts} AND rs.id > ?{min_id}))
             ORDER BY rs.published_at DESC, rs.id DESC
             LIMIT ?{limit}",
            search_clauses,
            max_ts = pattern_max_index + 1,
            max_id = pattern_max_index + 2,
            min_ts = pattern_max_index + 3,
            min_id = pattern_max_index + 4,
            limit = pattern_max_index + 5,
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let mut bindings = Vec::with_capacity(patterns.len() + 5);
        bindings.extend(
            patterns
                .iter()
                .map(|pattern| D1Type::Text(pattern.as_str())),
        );
        bindings.push(match max_timestamp {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match max_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match min_timestamp {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(match min_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        });
        bindings.push(D1Type::Integer(limit as i32));
        let pattern_max_index = patterns.len();
        db.prepare(&format!(
            "SELECT
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
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE ({})
               AND (?{max_ts} IS NULL
                    OR rs.published_at < ?{max_ts}
                    OR (rs.published_at = ?{max_ts} AND rs.id < ?{max_id}))
               AND (?{min_ts} IS NULL
                    OR rs.published_at > ?{min_ts}
                    OR (rs.published_at = ?{min_ts} AND rs.id > ?{min_id}))
             ORDER BY rs.published_at DESC, rs.id DESC
             LIMIT ?{limit}",
            search_clauses,
            max_ts = pattern_max_index + 1,
            max_id = pattern_max_index + 2,
            min_ts = pattern_max_index + 3,
            min_id = pattern_max_index + 4,
            limit = pattern_max_index + 5,
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    let values = result.results::<serde_json::Value>()?;
    let mut rows = Vec::with_capacity(values.len());
    for value in values {
        rows.push((
            RemoteStatusRow {
                id: value
                    .get("id")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                actor_uri: value
                    .get("actor_uri")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                object_uri: value
                    .get("object_uri")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                url: value
                    .get("url")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                in_reply_to_uri: value
                    .get("in_reply_to_uri")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                boost_of_uri: value
                    .get("boost_of_uri")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                quote_of_uri: value
                    .get("quote_of_uri")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                content_html: value
                    .get("content_html")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                spoiler_text: value
                    .get("spoiler_text")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                visibility: value
                    .get("visibility")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                sensitive: value
                    .get("sensitive")
                    .and_then(|field| field.as_i64())
                    .unwrap_or_default() as i32,
                language: value
                    .get("language")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                quote_state: value
                    .get("quote_state")
                    .and_then(|field| field.as_str())
                    .unwrap_or("accepted")
                    .to_owned(),
                published_at: value
                    .get("published_at")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            },
            RemoteActorRow::from_value(&value),
        ));
    }

    Ok(rows)
}
