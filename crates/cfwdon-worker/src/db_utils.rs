use super::{D1Database, Result};
use worker::d1::D1Type;

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

pub(crate) async fn count_rows_like(db: &D1Database, sql: &str, pattern: &str) -> Result<u64> {
    count_rows(db, sql, pattern).await
}
