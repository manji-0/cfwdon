use crate::{
    Error, Request, Response, Result, RouteContext, generate_entity_id, load_config,
    parse_optional_bool, require_authenticated_local_account,
};
use serde::Deserialize;
use std::collections::HashMap;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use worker::d1::D1Type;

const LOAD_LATEST_FILTER_UPDATED_AT_SQL: &str = "SELECT MAX(updated_at) AS updated_at
             FROM (
                 SELECT f.updated_at AS updated_at
                 FROM filters f
                 WHERE f.account_id = ?1
                 UNION ALL
                 SELECT k.updated_at AS updated_at
                 FROM filter_keywords k
                 JOIN filters f ON f.id = k.filter_id
                 WHERE f.account_id = ?1
                 UNION ALL
                 SELECT s.created_at AS updated_at
                 FROM filter_statuses s
                 JOIN filters f ON f.id = s.filter_id
                 WHERE f.account_id = ?1
             )";

#[derive(Debug, Deserialize)]
struct FilterRow {
    id: String,
    title: String,
    context_csv: String,
    expires_at: Option<String>,
    filter_action: String,
}

#[derive(Debug, Deserialize)]
struct FilterKeywordRow {
    id: String,
    filter_id: String,
    keyword: String,
    whole_word: i32,
}

#[derive(Debug, Deserialize)]
struct FilterStatusRow {
    id: String,
    #[serde(default)]
    filter_id: String,
    status_id: String,
}

#[derive(Debug, Deserialize)]
struct V1FilterRow {
    id: String,
    phrase: String,
    context_csv: String,
    expires_at: Option<String>,
    filter_action: String,
    whole_word: i32,
}

#[derive(Debug, Default)]
pub(crate) struct AccountFilterMatcher {
    filters: Vec<FilterRow>,
    keywords_by_filter_id: HashMap<String, Vec<FilterKeywordRow>>,
    statuses_by_filter_id: HashMap<String, Vec<FilterStatusRow>>,
}

#[derive(Debug, Default, Deserialize)]
struct KeywordInput {
    keyword: Option<String>,
    whole_word: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct V1FilterRequest {
    phrase: Option<String>,
    context: Option<Vec<String>>,
    expires_in: Option<i64>,
    irreversible: Option<bool>,
    whole_word: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct V2FilterRequest {
    title: Option<String>,
    context: Option<Vec<String>>,
    expires_in: Option<i64>,
    filter_action: Option<String>,
    #[serde(alias = "keywords_attributes")]
    keywords: Option<Vec<KeywordInput>>,
    phrase: Option<String>,
    whole_word: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct KeywordRequest {
    keyword: Option<String>,
    whole_word: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct StatusFilterRequest {
    status_id: Option<String>,
}

fn split_filter_context(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn normalize_contexts(contexts: Vec<String>) -> std::result::Result<Vec<String>, Error> {
    let mut normalized = Vec::new();
    for context in contexts {
        let context = context.trim().to_ascii_lowercase();
        if context.is_empty() {
            continue;
        }
        match context.as_str() {
            "home" | "notifications" | "public" | "thread" | "account" => {
                if !normalized.contains(&context) {
                    normalized.push(context);
                }
            }
            _ => {
                return Err(Error::RustError(
                    "context must be one of: home, notifications, public, thread, account"
                        .to_string(),
                ));
            }
        }
    }
    if normalized.is_empty() {
        return Err(Error::RustError(
            "at least one context is required".to_owned(),
        ));
    }
    Ok(normalized)
}

fn normalize_filter_action(value: Option<&str>) -> std::result::Result<String, Error> {
    let normalized = value.unwrap_or("warn").trim().to_ascii_lowercase();
    match normalized.as_str() {
        "warn" | "hide" | "blur" => Ok(normalized),
        _ => Err(Error::RustError(
            "filter_action must be one of: warn, hide, blur".to_owned(),
        )),
    }
}

pub(crate) async fn load_latest_filter_updated_at(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<Option<String>> {
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(LOAD_LATEST_FILTER_UPDATED_AT_SQL)
        .bind_refs(&account_id)?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.and_then(|value| {
        value
            .get("updated_at")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
    }))
}

fn expires_at_from_seconds(seconds: Option<i64>) -> std::result::Result<Option<String>, Error> {
    let Some(seconds) = seconds else {
        return Ok(None);
    };
    let expires_at = (OffsetDateTime::now_utc() + Duration::seconds(seconds))
        .format(&Rfc3339)
        .map_err(|error| Error::RustError(format!("failed to format expires_at: {error}")))?;
    Ok(Some(expires_at))
}

fn keyword_document(row: &FilterKeywordRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "keyword": row.keyword,
        "whole_word": row.whole_word != 0,
    })
}

fn status_filter_document(row: &FilterStatusRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "status_id": row.status_id,
    })
}

fn filter_summary_document(row: &FilterRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "title": row.title,
        "context": split_filter_context(&row.context_csv),
        "expires_at": row.expires_at,
        "filter_action": row.filter_action,
    })
}

fn v1_filter_document(row: &V1FilterRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "phrase": row.phrase,
        "context": split_filter_context(&row.context_csv),
        "expires_at": row.expires_at,
        "irreversible": row.filter_action == "hide",
        "whole_word": row.whole_word != 0,
    })
}

async fn v2_filter_document(db: &worker::D1Database, row: &FilterRow) -> Result<serde_json::Value> {
    let keywords = list_filter_keywords(db, &row.id)
        .await?
        .into_iter()
        .map(|row| keyword_document(&row))
        .collect::<Vec<_>>();
    let statuses = list_filter_statuses(db, &row.id)
        .await?
        .into_iter()
        .map(|row| status_filter_document(&row))
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "id": row.id,
        "title": row.title,
        "context": split_filter_context(&row.context_csv),
        "expires_at": row.expires_at,
        "filter_action": row.filter_action,
        "keywords": keywords,
        "statuses": statuses,
    }))
}

async fn list_filters(db: &worker::D1Database, account_id: &str) -> Result<Vec<FilterRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT id, title, context_csv, expires_at, filter_action
             FROM filters
             WHERE account_id = ?1
             ORDER BY created_at DESC, id DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<FilterRow>()
}

async fn find_filter(
    db: &worker::D1Database,
    account_id: &str,
    filter_id: &str,
) -> Result<Option<FilterRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(filter_id)];
    db.prepare(
        "SELECT id, title, context_csv, expires_at, filter_action
         FROM filters
         WHERE account_id = ?1
           AND id = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<FilterRow>(None)
    .await
}

async fn list_filter_keywords(
    db: &worker::D1Database,
    filter_id: &str,
) -> Result<Vec<FilterKeywordRow>> {
    let filter_id = D1Type::Text(filter_id);
    let result = db
        .prepare(
            "SELECT id, filter_id, keyword, whole_word
             FROM filter_keywords
             WHERE filter_id = ?1
             ORDER BY created_at ASC, id ASC",
        )
        .bind_refs(&filter_id)?
        .all()
        .await?;
    result.results::<FilterKeywordRow>()
}

async fn list_filter_keywords_for_filters(
    db: &worker::D1Database,
    filters: &[FilterRow],
) -> Result<HashMap<String, Vec<FilterKeywordRow>>> {
    if filters.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = (1..=filters.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT id, filter_id, keyword, whole_word
         FROM filter_keywords
         WHERE filter_id IN ({placeholders})
         ORDER BY filter_id ASC, created_at ASC, id ASC"
    );
    let bindings = filters
        .iter()
        .map(|filter| D1Type::Text(filter.id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
    let mut by_filter_id = HashMap::new();
    for keyword in result.results::<FilterKeywordRow>()? {
        by_filter_id
            .entry(keyword.filter_id.clone())
            .or_insert_with(Vec::new)
            .push(keyword);
    }

    Ok(by_filter_id)
}

async fn find_filter_keyword(
    db: &worker::D1Database,
    account_id: &str,
    keyword_id: &str,
) -> Result<Option<FilterKeywordRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(keyword_id)];
    db.prepare(
        "SELECT k.id, k.filter_id, k.keyword, k.whole_word
         FROM filter_keywords k
         JOIN filters f ON f.id = k.filter_id
         WHERE f.account_id = ?1
           AND k.id = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<FilterKeywordRow>(None)
    .await
}

async fn list_filter_statuses(
    db: &worker::D1Database,
    filter_id: &str,
) -> Result<Vec<FilterStatusRow>> {
    let filter_id = D1Type::Text(filter_id);
    let result = db
        .prepare(
            "SELECT id, status_id
             FROM filter_statuses
             WHERE filter_id = ?1
             ORDER BY created_at ASC, id ASC",
        )
        .bind_refs(&filter_id)?
        .all()
        .await?;
    result.results::<FilterStatusRow>()
}

async fn list_filter_statuses_for_filters(
    db: &worker::D1Database,
    filters: &[FilterRow],
) -> Result<HashMap<String, Vec<FilterStatusRow>>> {
    if filters.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = (1..=filters.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT id, filter_id, status_id
         FROM filter_statuses
         WHERE filter_id IN ({placeholders})
         ORDER BY filter_id ASC, created_at ASC, id ASC"
    );
    let bindings = filters
        .iter()
        .map(|filter| D1Type::Text(filter.id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
    let mut by_filter_id = HashMap::new();
    for status in result.results::<FilterStatusRow>()? {
        by_filter_id
            .entry(status.filter_id.clone())
            .or_insert_with(Vec::new)
            .push(status);
    }

    Ok(by_filter_id)
}

async fn find_filter_status(
    db: &worker::D1Database,
    account_id: &str,
    status_filter_id: &str,
) -> Result<Option<FilterStatusRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(status_filter_id)];
    db.prepare(
        "SELECT s.id, s.status_id
         FROM filter_statuses s
         JOIN filters f ON f.id = s.filter_id
         WHERE f.account_id = ?1
           AND s.id = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<FilterStatusRow>(None)
    .await
}

async fn list_v1_filters(db: &worker::D1Database, account_id: &str) -> Result<Vec<V1FilterRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT k.id, k.keyword AS phrase, f.context_csv, f.expires_at, f.filter_action, k.whole_word
             FROM filter_keywords k
             JOIN filters f ON f.id = k.filter_id
             WHERE f.account_id = ?1
             ORDER BY f.created_at DESC, k.created_at ASC, k.id ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<V1FilterRow>()
}

async fn find_v1_filter(
    db: &worker::D1Database,
    account_id: &str,
    keyword_id: &str,
) -> Result<Option<V1FilterRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(keyword_id)];
    db.prepare(
        "SELECT k.id, k.keyword AS phrase, f.context_csv, f.expires_at, f.filter_action, k.whole_word
         FROM filter_keywords k
         JOIN filters f ON f.id = k.filter_id
         WHERE f.account_id = ?1
           AND k.id = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<V1FilterRow>(None)
    .await
}

async fn create_filter_row(
    db: &worker::D1Database,
    account_id: &str,
    title: &str,
    contexts: &[String],
    expires_at: Option<&str>,
    filter_action: &str,
) -> Result<String> {
    let filter_id = generate_entity_id(16)?;
    let context_csv = contexts.join(",");
    let bindings = [
        D1Type::Text(filter_id.as_str()),
        D1Type::Text(account_id),
        D1Type::Text(title),
        D1Type::Text(context_csv.as_str()),
        expires_at.map(D1Type::Text).unwrap_or(D1Type::Null),
        D1Type::Text(filter_action),
    ];
    db.prepare(
        "INSERT INTO filters (
            id, account_id, title, context_csv, expires_at, filter_action, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(filter_id)
}

async fn update_filter_row(
    db: &worker::D1Database,
    account_id: &str,
    filter_id: &str,
    title: &str,
    contexts: &[String],
    expires_at: Option<&str>,
    filter_action: &str,
) -> Result<bool> {
    let context_csv = contexts.join(",");
    let bindings = [
        D1Type::Text(title),
        D1Type::Text(context_csv.as_str()),
        expires_at.map(D1Type::Text).unwrap_or(D1Type::Null),
        D1Type::Text(filter_action),
        D1Type::Text(account_id),
        D1Type::Text(filter_id),
    ];
    let result = db
        .prepare(
            "UPDATE filters
             SET title = ?1,
                 context_csv = ?2,
                 expires_at = ?3,
                 filter_action = ?4,
                 updated_at = CURRENT_TIMESTAMP
             WHERE account_id = ?5
               AND id = ?6",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    did_change(&result)
}

async fn delete_filter_row(
    db: &worker::D1Database,
    account_id: &str,
    filter_id: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(filter_id)];
    db.prepare("DELETE FROM filter_keywords WHERE filter_id = ?1")
        .bind_refs(&bindings)?
        .run()
        .await?;
    db.prepare("DELETE FROM filter_statuses WHERE filter_id = ?1")
        .bind_refs(&bindings)?
        .run()
        .await?;
    let bindings = [D1Type::Text(account_id), D1Type::Text(filter_id)];
    let result = db
        .prepare("DELETE FROM filters WHERE account_id = ?1 AND id = ?2")
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    did_change(&result)
}

async fn replace_filter_keywords(
    db: &worker::D1Database,
    filter_id: &str,
    keywords: &[(String, bool)],
) -> Result<()> {
    let filter_id_binding = D1Type::Text(filter_id);
    db.prepare("DELETE FROM filter_keywords WHERE filter_id = ?1")
        .bind_refs(&filter_id_binding)?
        .run()
        .await?;

    for (keyword, whole_word) in keywords {
        let keyword_id = generate_entity_id(16)?;
        let bindings = [
            D1Type::Text(keyword_id.as_str()),
            D1Type::Text(filter_id),
            D1Type::Text(keyword.as_str()),
            D1Type::Integer(if *whole_word { 1 } else { 0 }),
        ];
        db.prepare(
            "INSERT INTO filter_keywords (id, filter_id, keyword, whole_word, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }
    Ok(())
}

async fn create_filter_keyword_row(
    db: &worker::D1Database,
    filter_id: &str,
    keyword: &str,
    whole_word: bool,
) -> Result<String> {
    let keyword_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(keyword_id.as_str()),
        D1Type::Text(filter_id),
        D1Type::Text(keyword),
        D1Type::Integer(if whole_word { 1 } else { 0 }),
    ];
    db.prepare(
        "INSERT INTO filter_keywords (id, filter_id, keyword, whole_word, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(keyword_id)
}

async fn update_filter_keyword_row(
    db: &worker::D1Database,
    keyword_id: &str,
    keyword: &str,
    whole_word: bool,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(keyword),
        D1Type::Integer(if whole_word { 1 } else { 0 }),
        D1Type::Text(keyword_id),
    ];
    let result = db
        .prepare(
            "UPDATE filter_keywords
             SET keyword = ?1,
                 whole_word = ?2,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?3",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    did_change(&result)
}

async fn delete_filter_keyword_row(
    db: &worker::D1Database,
    keyword_id: &str,
) -> Result<Option<String>> {
    let keyword_id_binding = D1Type::Text(keyword_id);
    let row = db
        .prepare("SELECT filter_id FROM filter_keywords WHERE id = ?1 LIMIT 1")
        .bind_refs(&keyword_id_binding)?
        .first::<serde_json::Value>(None)
        .await?;
    let filter_id = row
        .as_ref()
        .and_then(|row| row.get("filter_id"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let result = db
        .prepare("DELETE FROM filter_keywords WHERE id = ?1")
        .bind_refs(&keyword_id_binding)?
        .run()
        .await?;
    if !did_change(&result)? {
        return Ok(None);
    }
    Ok(filter_id)
}

async fn create_filter_status_row(
    db: &worker::D1Database,
    filter_id: &str,
    status_id: &str,
) -> Result<String> {
    let status_filter_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(status_filter_id.as_str()),
        D1Type::Text(filter_id),
        D1Type::Text(status_id),
    ];
    db.prepare(
        "INSERT OR IGNORE INTO filter_statuses (id, filter_id, status_id, created_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(status_filter_id)
}

async fn delete_filter_status_row(db: &worker::D1Database, status_filter_id: &str) -> Result<bool> {
    let status_filter_id_binding = D1Type::Text(status_filter_id);
    let result = db
        .prepare("DELETE FROM filter_statuses WHERE id = ?1")
        .bind_refs(&status_filter_id_binding)?
        .run()
        .await?;
    did_change(&result)
}

fn did_change(result: &worker::d1::D1Result) -> Result<bool> {
    Ok(result
        .meta()?
        .and_then(|meta| {
            meta.changed_db
                .or_else(|| meta.changes.map(|changes| changes > 0))
        })
        .unwrap_or(false))
}

fn phrase_matches_text(text: &str, phrase: &str, whole_word: bool) -> bool {
    if phrase.is_empty() {
        return false;
    }
    if !whole_word {
        return text.contains(phrase);
    }

    let mut start = 0usize;
    while let Some(relative_idx) = text[start..].find(phrase) {
        let idx = start + relative_idx;
        let before = text[..idx].chars().next_back();
        let after = text[idx + phrase.len()..].chars().next();
        let before_ok = before.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_');
        let after_ok = after.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_');
        if before_ok && after_ok {
            return true;
        }
        start = idx + phrase.len();
    }
    false
}

fn filter_is_expired(expires_at: Option<&str>) -> bool {
    expires_at.is_some_and(|value| {
        OffsetDateTime::parse(value, &Rfc3339)
            .map(|datetime| datetime <= OffsetDateTime::now_utc())
            .unwrap_or(false)
    })
}

impl AccountFilterMatcher {
    pub(crate) fn filtered_status(
        &self,
        status_id: &str,
        text: &str,
        spoiler_text: &str,
    ) -> Vec<serde_json::Value> {
        if self.filters.is_empty() {
            return Vec::new();
        }

        let haystack = format!("{}\n{}", text, spoiler_text).to_ascii_lowercase();
        let mut filtered = Vec::new();

        for filter in &self.filters {
            if filter_is_expired(filter.expires_at.as_deref()) {
                continue;
            }

            let keyword_matches = self
                .keywords_by_filter_id
                .get(&filter.id)
                .into_iter()
                .flatten()
                .filter_map(|keyword| {
                    let normalized = keyword.keyword.trim().to_ascii_lowercase();
                    phrase_matches_text(&haystack, &normalized, keyword.whole_word != 0)
                        .then_some(keyword.keyword.clone())
                })
                .collect::<Vec<_>>();
            let status_matches = self
                .statuses_by_filter_id
                .get(&filter.id)
                .into_iter()
                .flatten()
                .filter_map(|status_filter| {
                    (status_filter.status_id == status_id)
                        .then_some(status_filter.status_id.clone())
                })
                .collect::<Vec<_>>();

            if keyword_matches.is_empty() && status_matches.is_empty() {
                continue;
            }

            filtered.push(serde_json::json!({
                "filter": filter_summary_document(filter),
                "keyword_matches": if keyword_matches.is_empty() { serde_json::Value::Null } else { serde_json::json!(keyword_matches) },
                "status_matches": if status_matches.is_empty() { serde_json::Value::Null } else { serde_json::json!(status_matches) },
            }));
        }

        filtered
    }
}

pub(crate) async fn load_account_filter_matcher(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<AccountFilterMatcher> {
    let filters = list_filters(db, account_id).await?;
    if filters.is_empty() {
        return Ok(AccountFilterMatcher::default());
    }

    let (keywords_by_filter_id, statuses_by_filter_id) = futures_util::try_join!(
        list_filter_keywords_for_filters(db, &filters),
        list_filter_statuses_for_filters(db, &filters),
    )?;

    Ok(AccountFilterMatcher {
        filters,
        keywords_by_filter_id,
        statuses_by_filter_id,
    })
}

pub(crate) async fn load_status_filtered(
    db: &worker::D1Database,
    account_id: &str,
    status_id: &str,
    text: &str,
    spoiler_text: &str,
) -> Result<Vec<serde_json::Value>> {
    Ok(load_account_filter_matcher(db, account_id)
        .await?
        .filtered_status(status_id, text, spoiler_text))
}

fn normalize_keyword(value: Option<&str>) -> std::result::Result<String, Error> {
    let keyword = value.unwrap_or_default().trim().to_owned();
    if keyword.is_empty() {
        return Err(Error::RustError("keyword must not be empty".to_owned()));
    }
    Ok(keyword)
}

fn parse_i64(value: Option<&str>, field: &str) -> std::result::Result<Option<i64>, Error> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => value
            .parse::<i64>()
            .map(Some)
            .map_err(|_| Error::RustError(format!("{field} must be an integer"))),
    }
}

fn parse_form_keyword_entries(body: &str) -> std::result::Result<Vec<KeywordInput>, Error> {
    let mut keywords = Vec::<KeywordInput>::new();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        let key = key.as_ref();
        if key.starts_with("keywords_attributes") && key.ends_with("[keyword]") {
            keywords.push(KeywordInput {
                keyword: Some(value.into_owned()),
                whole_word: None,
            });
            continue;
        }
        if key.starts_with("keywords_attributes") && key.ends_with("[whole_word]") {
            let whole_word = parse_optional_bool(Some(value.as_ref()))
                .map_err(Error::RustError)?
                .unwrap_or(true);
            if let Some(last) = keywords.last_mut() {
                last.whole_word = Some(whole_word);
            } else {
                keywords.push(KeywordInput {
                    keyword: None,
                    whole_word: Some(whole_word),
                });
            }
        }
    }
    Ok(keywords)
}

async fn parse_v1_filter_request(req: &mut Request) -> std::result::Result<V1FilterRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        return req
            .json::<V1FilterRequest>()
            .await
            .map_err(|error| Error::RustError(format!("invalid JSON filter payload: {error}")));
    }

    let body = req
        .text()
        .await
        .map_err(|error| Error::RustError(format!("invalid filter payload: {error}")))?;
    let mut request = V1FilterRequest::default();
    let mut contexts = Vec::new();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        match key.as_ref() {
            "phrase" => request.phrase = Some(value.into_owned()),
            "context[]" | "context" => contexts.push(value.into_owned()),
            "expires_in" => request.expires_in = parse_i64(Some(value.as_ref()), "expires_in")?,
            "irreversible" => {
                request.irreversible =
                    parse_optional_bool(Some(value.as_ref())).map_err(Error::RustError)?
            }
            "whole_word" => {
                request.whole_word =
                    parse_optional_bool(Some(value.as_ref())).map_err(Error::RustError)?
            }
            _ => {}
        }
    }
    if !contexts.is_empty() {
        request.context = Some(contexts);
    }
    Ok(request)
}

async fn parse_v2_filter_request(req: &mut Request) -> std::result::Result<V2FilterRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        return req
            .json::<V2FilterRequest>()
            .await
            .map_err(|error| Error::RustError(format!("invalid JSON v2 filter payload: {error}")));
    }

    let body = req
        .text()
        .await
        .map_err(|error| Error::RustError(format!("invalid v2 filter payload: {error}")))?;
    let mut request = V2FilterRequest::default();
    let mut contexts = Vec::new();
    let mut keywords = parse_form_keyword_entries(&body)?;
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        match key.as_ref() {
            "title" => request.title = Some(value.into_owned()),
            "phrase" => request.phrase = Some(value.into_owned()),
            "context[]" | "context" => contexts.push(value.into_owned()),
            "expires_in" => request.expires_in = parse_i64(Some(value.as_ref()), "expires_in")?,
            "filter_action" => request.filter_action = Some(value.into_owned()),
            "whole_word" => {
                request.whole_word =
                    parse_optional_bool(Some(value.as_ref())).map_err(Error::RustError)?
            }
            _ => {}
        }
    }
    if !contexts.is_empty() {
        request.context = Some(contexts);
    }
    if !keywords.is_empty() {
        request.keywords = Some(std::mem::take(&mut keywords));
    }
    Ok(request)
}

async fn parse_keyword_request(req: &mut Request) -> std::result::Result<KeywordRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("application/json") {
        return req
            .json::<KeywordRequest>()
            .await
            .map_err(|error| Error::RustError(format!("invalid JSON keyword payload: {error}")));
    }
    let body = req
        .text()
        .await
        .map_err(|error| Error::RustError(format!("invalid keyword payload: {error}")))?;
    let mut request = KeywordRequest::default();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        match key.as_ref() {
            "keyword" => request.keyword = Some(value.into_owned()),
            "whole_word" => {
                request.whole_word =
                    parse_optional_bool(Some(value.as_ref())).map_err(Error::RustError)?
            }
            _ => {}
        }
    }
    Ok(request)
}

async fn parse_status_filter_request(
    req: &mut Request,
) -> std::result::Result<StatusFilterRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("application/json") {
        return req.json::<StatusFilterRequest>().await.map_err(|error| {
            Error::RustError(format!("invalid JSON status filter payload: {error}"))
        });
    }
    let body = req
        .text()
        .await
        .map_err(|error| Error::RustError(format!("invalid status filter payload: {error}")))?;
    let mut request = StatusFilterRequest::default();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        if key == "status_id" {
            request.status_id = Some(value.into_owned());
        }
    }
    Ok(request)
}

pub(crate) async fn filters_v1_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let filters = list_v1_filters(&db, &viewer.id)
        .await?
        .into_iter()
        .map(|row| v1_filter_document(&row))
        .collect::<Vec<_>>();
    Response::from_json(&filters)
}

pub(crate) async fn filter_v1_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;

    match find_v1_filter(&db, &viewer.id, &filter_id).await? {
        Some(row) => Response::from_json(&v1_filter_document(&row)),
        None => Response::error("filter not found", 404),
    }
}

pub(crate) async fn create_filter_v1_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request = parse_v1_filter_request(req).await?;
    let phrase = normalize_keyword(request.phrase.as_deref())?;
    let contexts = normalize_contexts(request.context.unwrap_or_default())?;
    let expires_at = expires_at_from_seconds(request.expires_in)?;
    let filter_action = if request.irreversible.unwrap_or(false) {
        "hide".to_owned()
    } else {
        "warn".to_owned()
    };
    let whole_word = request.whole_word.unwrap_or(false);

    let filter_id = create_filter_row(
        &db,
        &viewer.id,
        &phrase,
        &contexts,
        expires_at.as_deref(),
        &filter_action,
    )
    .await?;
    replace_filter_keywords(&db, &filter_id, &[(phrase.clone(), whole_word)]).await?;
    let Some(row) = list_v1_filters(&db, &viewer.id)
        .await?
        .into_iter()
        .find(|row| row.phrase == phrase)
    else {
        return Response::error("failed to load filter", 500);
    };
    Response::from_json(&v1_filter_document(&row))
}

pub(crate) async fn update_filter_v1_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    let Some(existing_keyword) = find_filter_keyword(&db, &viewer.id, &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    let Some(existing_filter) = find_filter(&db, &viewer.id, &existing_keyword.filter_id).await?
    else {
        return Response::error("filter not found", 404);
    };

    let request = parse_v1_filter_request(req).await?;
    let keyword_count = list_filter_keywords(&db, &existing_filter.id).await?.len();
    if keyword_count > 1
        && (request.context.is_some()
            || request.expires_in.is_some()
            || request.irreversible.is_some())
    {
        return Response::error(
            "cannot update context, expires_in, or irreversible on a v1 filter backed by multiple keywords",
            422,
        );
    }
    let phrase = match request.phrase.as_deref() {
        Some(value) => normalize_keyword(Some(value))?,
        None => existing_keyword.keyword.clone(),
    };
    let contexts = match request.context {
        Some(context) => normalize_contexts(context)?,
        None => split_filter_context(&existing_filter.context_csv),
    };
    let expires_at = match request.expires_in {
        Some(seconds) => expires_at_from_seconds(Some(seconds))?,
        None => existing_filter.expires_at.clone(),
    };
    let filter_action = if request
        .irreversible
        .unwrap_or(existing_filter.filter_action == "hide")
    {
        "hide".to_owned()
    } else {
        "warn".to_owned()
    };
    let whole_word = request
        .whole_word
        .unwrap_or(existing_keyword.whole_word != 0);

    update_filter_row(
        &db,
        &viewer.id,
        &existing_filter.id,
        &phrase,
        &contexts,
        expires_at.as_deref(),
        &filter_action,
    )
    .await?;
    update_filter_keyword_row(&db, &existing_keyword.id, &phrase, whole_word).await?;

    let Some(row) = find_v1_filter(&db, &viewer.id, &existing_keyword.id).await? else {
        return Response::error("filter not found", 404);
    };
    Response::from_json(&v1_filter_document(&row))
}

pub(crate) async fn delete_filter_v1_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;

    let Some(keyword) = find_filter_keyword(&db, &viewer.id, &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    delete_filter_keyword_row(&db, &keyword.id).await?;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn filters_v2_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mut response = Vec::new();
    for row in list_filters(&db, &viewer.id).await? {
        response.push(v2_filter_document(&db, &row).await?);
    }
    Response::from_json(&response)
}

pub(crate) async fn filter_v2_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;

    match find_filter(&db, &viewer.id, &filter_id).await? {
        Some(row) => Response::from_json(&v2_filter_document(&db, &row).await?),
        None => Response::error("filter not found", 404),
    }
}

pub(crate) async fn create_filter_v2_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request = parse_v2_filter_request(req).await?;
    let contexts = normalize_contexts(request.context.unwrap_or_default())?;
    let filter_action = normalize_filter_action(request.filter_action.as_deref())?;
    let expires_at = expires_at_from_seconds(request.expires_in)?;
    let keywords = match request.keywords {
        Some(entries) => entries
            .into_iter()
            .filter_map(|entry| {
                entry.keyword.map(|keyword| {
                    let keyword = keyword.trim().to_owned();
                    (!keyword.is_empty()).then_some((keyword, entry.whole_word.unwrap_or(false)))
                })
            })
            .flatten()
            .collect::<Vec<_>>(),
        None => match request
            .phrase
            .as_deref()
            .or(request.title.as_deref())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            Some(value) => vec![(value.to_owned(), request.whole_word.unwrap_or(false))],
            None => Vec::new(),
        },
    };
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| keywords.first().map(|(keyword, _)| keyword.clone()))
        .ok_or_else(|| Error::RustError("title or keyword is required".to_owned()))?;

    let filter_id = create_filter_row(
        &db,
        &viewer.id,
        &title,
        &contexts,
        expires_at.as_deref(),
        &filter_action,
    )
    .await?;
    replace_filter_keywords(&db, &filter_id, &keywords).await?;
    let Some(row) = find_filter(&db, &viewer.id, &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    Response::from_json(&v2_filter_document(&db, &row).await?)
}

pub(crate) async fn update_filter_v2_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    let Some(existing) = find_filter(&db, &viewer.id, &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    let request = parse_v2_filter_request(req).await?;

    let contexts = match request.context {
        Some(contexts) => normalize_contexts(contexts)?,
        None => split_filter_context(&existing.context_csv),
    };
    let filter_action = match request.filter_action.as_deref() {
        Some(value) => normalize_filter_action(Some(value))?,
        None => existing.filter_action.clone(),
    };
    let expires_at = match request.expires_in {
        Some(seconds) => expires_at_from_seconds(Some(seconds))?,
        None => existing.expires_at.clone(),
    };
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(existing.title.clone());

    update_filter_row(
        &db,
        &viewer.id,
        &filter_id,
        &title,
        &contexts,
        expires_at.as_deref(),
        &filter_action,
    )
    .await?;

    if let Some(entries) = request.keywords {
        let keywords = entries
            .into_iter()
            .filter_map(|entry| {
                entry.keyword.map(|keyword| {
                    let keyword = keyword.trim().to_owned();
                    (!keyword.is_empty()).then_some((keyword, entry.whole_word.unwrap_or(false)))
                })
            })
            .flatten()
            .collect::<Vec<_>>();
        replace_filter_keywords(&db, &filter_id, &keywords).await?;
    }

    let Some(row) = find_filter(&db, &viewer.id, &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    Response::from_json(&v2_filter_document(&db, &row).await?)
}

pub(crate) async fn delete_filter_v2_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if !delete_filter_row(&db, &viewer.id, &filter_id).await? {
        return Response::error("filter not found", 404);
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn filter_keywords_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if find_filter(&db, &viewer.id, &filter_id).await?.is_none() {
        return Response::error("filter not found", 404);
    }
    let response = list_filter_keywords(&db, &filter_id)
        .await?
        .into_iter()
        .map(|row| keyword_document(&row))
        .collect::<Vec<_>>();
    Response::from_json(&response)
}

pub(crate) async fn create_filter_keyword_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if find_filter(&db, &viewer.id, &filter_id).await?.is_none() {
        return Response::error("filter not found", 404);
    }
    let request = parse_keyword_request(req).await?;
    let keyword = normalize_keyword(request.keyword.as_deref())?;
    let whole_word = request.whole_word.unwrap_or(false);
    let keyword_id = create_filter_keyword_row(&db, &filter_id, &keyword, whole_word).await?;
    Response::from_json(&serde_json::json!({
        "id": keyword_id,
        "keyword": keyword,
        "whole_word": whole_word,
    }))
}

pub(crate) async fn filter_keyword_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let keyword_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing keyword id route parameter".to_owned()))?;
    match find_filter_keyword(&db, &viewer.id, &keyword_id).await? {
        Some(row) => Response::from_json(&keyword_document(&row)),
        None => Response::error("filter keyword not found", 404),
    }
}

pub(crate) async fn update_filter_keyword_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let keyword_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing keyword id route parameter".to_owned()))?;
    let Some(existing) = find_filter_keyword(&db, &viewer.id, &keyword_id).await? else {
        return Response::error("filter keyword not found", 404);
    };
    let request = parse_keyword_request(req).await?;
    let keyword = match request.keyword.as_deref() {
        Some(value) => normalize_keyword(Some(value))?,
        None => existing.keyword.clone(),
    };
    let whole_word = request.whole_word.unwrap_or(existing.whole_word != 0);
    update_filter_keyword_row(&db, &keyword_id, &keyword, whole_word).await?;
    Response::from_json(&serde_json::json!({
        "id": keyword_id,
        "keyword": keyword,
        "whole_word": whole_word,
    }))
}

pub(crate) async fn delete_filter_keyword_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let keyword_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing keyword id route parameter".to_owned()))?;
    let Some(keyword) = find_filter_keyword(&db, &viewer.id, &keyword_id).await? else {
        return Response::error("filter keyword not found", 404);
    };
    delete_filter_keyword_row(&db, &keyword.id).await?;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn filter_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if find_filter(&db, &viewer.id, &filter_id).await?.is_none() {
        return Response::error("filter not found", 404);
    }
    let response = list_filter_statuses(&db, &filter_id)
        .await?
        .into_iter()
        .map(|row| status_filter_document(&row))
        .collect::<Vec<_>>();
    Response::from_json(&response)
}

pub(crate) async fn create_filter_status_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if find_filter(&db, &viewer.id, &filter_id).await?.is_none() {
        return Response::error("filter not found", 404);
    }
    let request = parse_status_filter_request(req).await?;
    let status_id = normalize_keyword(request.status_id.as_deref())?;
    let status_filter_id = create_filter_status_row(&db, &filter_id, &status_id).await?;
    Response::from_json(&serde_json::json!({
        "id": status_filter_id,
        "status_id": status_id,
    }))
}

pub(crate) async fn filter_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let status_filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter status id route parameter".to_owned()))?;
    match find_filter_status(&db, &viewer.id, &status_filter_id).await? {
        Some(row) => Response::from_json(&status_filter_document(&row)),
        None => Response::error("filter status not found", 404),
    }
}

pub(crate) async fn delete_filter_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let status_filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter status id route parameter".to_owned()))?;
    if find_filter_status(&db, &viewer.id, &status_filter_id)
        .await?
        .is_none()
    {
        return Response::error("filter status not found", 404);
    }
    delete_filter_status_row(&db, &status_filter_id).await?;
    Response::from_json(&serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_filter_context_discards_empty_values() {
        assert_eq!(
            split_filter_context("home, notifications,,thread"),
            vec!["home", "notifications", "thread"]
        );
    }

    #[test]
    fn normalize_contexts_trims_lowercases_and_deduplicates() {
        assert_eq!(
            normalize_contexts(vec![
                " Home ".to_owned(),
                "PUBLIC".to_owned(),
                "home".to_owned(),
                " thread ".to_owned(),
            ])
            .unwrap(),
            vec!["home", "public", "thread"]
        );
    }

    #[test]
    fn normalize_contexts_rejects_empty_and_unknown_values() {
        assert!(normalize_contexts(vec![" ".to_owned()]).is_err());
        assert!(normalize_contexts(vec!["home".to_owned(), "unknown".to_owned()]).is_err());
    }

    #[test]
    fn normalize_filter_action_accepts_current_values() {
        assert_eq!(normalize_filter_action(Some("warn")).unwrap(), "warn");
        assert_eq!(normalize_filter_action(Some("hide")).unwrap(), "hide");
        assert_eq!(normalize_filter_action(Some("blur")).unwrap(), "blur");
    }

    #[test]
    fn latest_filter_updated_at_uses_filter_status_created_at() {
        assert!(LOAD_LATEST_FILTER_UPDATED_AT_SQL.contains("s.created_at AS updated_at"));
        assert!(!LOAD_LATEST_FILTER_UPDATED_AT_SQL.contains("s.updated_at"));
    }

    #[test]
    fn parse_form_keyword_entries_pairs_keyword_and_whole_word() {
        let rows = parse_form_keyword_entries(
            "keywords_attributes[][keyword]=mute&keywords_attributes[][whole_word]=false",
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].keyword.as_deref(), Some("mute"));
        assert_eq!(rows[0].whole_word, Some(false));
    }

    #[test]
    fn phrase_matches_text_respects_whole_word_boundaries() {
        assert!(phrase_matches_text("hello world", "world", true));
        assert!(!phrase_matches_text("helloworld", "world", true));
        assert!(phrase_matches_text("helloworld", "world", false));
    }

    #[test]
    fn account_filter_matcher_matches_keywords_and_status_ids() {
        let matcher = AccountFilterMatcher {
            filters: vec![FilterRow {
                id: "filter-1".to_owned(),
                title: "Quiet words".to_owned(),
                context_csv: "home".to_owned(),
                expires_at: None,
                filter_action: "warn".to_owned(),
            }],
            keywords_by_filter_id: HashMap::from([(
                "filter-1".to_owned(),
                vec![FilterKeywordRow {
                    id: "keyword-1".to_owned(),
                    filter_id: "filter-1".to_owned(),
                    keyword: "launch".to_owned(),
                    whole_word: 1,
                }],
            )]),
            statuses_by_filter_id: HashMap::from([(
                "filter-1".to_owned(),
                vec![FilterStatusRow {
                    id: "status-filter-1".to_owned(),
                    filter_id: "filter-1".to_owned(),
                    status_id: "status-1".to_owned(),
                }],
            )]),
        };

        let filtered = matcher.filtered_status("status-1", "Launch day", "");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0]["filter"]["id"], serde_json::json!("filter-1"));
        assert_eq!(
            filtered[0]["keyword_matches"],
            serde_json::json!(["launch"])
        );
        assert_eq!(
            filtered[0]["status_matches"],
            serde_json::json!(["status-1"])
        );
    }
}
