use super::{FilterKeywordRow, FilterRow, FilterStatusRow, V1FilterRow};
use crate::{Result, generate_entity_id};
use std::collections::HashMap;
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

pub(crate) async fn load_latest_filter_updated_at(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn list_filters(
    db: &crate::D1Database,
    account_id: &str,
) -> Result<Vec<FilterRow>> {
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
    crate::d1_results::<FilterRow>(&result)
}

pub(in crate::filters) async fn find_filter(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn list_filter_keywords(
    db: &crate::D1Database,
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
    crate::d1_results::<FilterKeywordRow>(&result)
}

pub(in crate::filters) async fn list_filter_keywords_for_filters(
    db: &crate::D1Database,
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
    for keyword in crate::d1_results::<FilterKeywordRow>(&result)? {
        by_filter_id
            .entry(keyword.filter_id.clone())
            .or_insert_with(Vec::new)
            .push(keyword);
    }

    Ok(by_filter_id)
}

pub(in crate::filters) async fn find_filter_keyword(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn list_filter_statuses(
    db: &crate::D1Database,
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
    crate::d1_results::<FilterStatusRow>(&result)
}

pub(in crate::filters) async fn list_filter_statuses_for_filters(
    db: &crate::D1Database,
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
    for status in crate::d1_results::<FilterStatusRow>(&result)? {
        by_filter_id
            .entry(status.filter_id.clone())
            .or_insert_with(Vec::new)
            .push(status);
    }

    Ok(by_filter_id)
}

pub(in crate::filters) async fn find_filter_status(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn list_v1_filters(
    db: &crate::D1Database,
    account_id: &str,
) -> Result<Vec<V1FilterRow>> {
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
    crate::d1_results::<V1FilterRow>(&result)
}

pub(in crate::filters) async fn find_v1_filter(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn create_filter_row(
    db: &crate::D1Database,
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
    crate::invalidate_account_capabilities(account_id).await;
    Ok(filter_id)
}

pub(in crate::filters) async fn update_filter_row(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn delete_filter_row(
    db: &crate::D1Database,
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
    let changed = did_change(&result)?;
    if changed {
        crate::invalidate_account_capabilities(account_id).await;
    }
    Ok(changed)
}

pub(in crate::filters) async fn replace_filter_keywords(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn create_filter_keyword_row(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn update_filter_keyword_row(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn delete_filter_keyword_row(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn create_filter_status_row(
    db: &crate::D1Database,
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

pub(in crate::filters) async fn delete_filter_status_row(
    db: &crate::D1Database,
    status_filter_id: &str,
) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_filter_updated_at_uses_filter_status_created_at() {
        assert!(LOAD_LATEST_FILTER_UPDATED_AT_SQL.contains("s.created_at AS updated_at"));
        assert!(!LOAD_LATEST_FILTER_UPDATED_AT_SQL.contains("s.updated_at"));
    }
}
