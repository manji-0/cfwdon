use super::{D1Database, Result};
use worker::d1::D1Type;

pub(crate) fn sql_placeholders(start: usize, len: usize) -> String {
    (start..start + len)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn unique_ordered_refs<'a>(values: &'a [String]) -> Vec<&'a String> {
    let mut seen = std::collections::HashSet::new();
    values
        .iter()
        .filter(|value| seen.insert(value.as_str()))
        .collect()
}

pub(crate) async fn count_rows(db: &D1Database, sql: &str, value: &str) -> Result<u64> {
    let value = D1Type::Text(value);
    let row = db
        .prepare(sql)
        .bind_refs(&value)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|row| row.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0))
}
