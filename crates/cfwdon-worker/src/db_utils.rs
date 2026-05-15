use super::{D1Database, Result};
use worker::d1::D1Type;

pub(crate) fn sql_placeholders(start: usize, len: usize) -> String {
    (start..start + len)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn unique_ordered_refs(values: &[String]) -> Vec<&String> {
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

#[cfg(test)]
mod tests {
    use super::unique_ordered_refs;

    #[test]
    fn unique_ordered_refs_preserves_first_seen_order() {
        let values = vec![
            "alpha".to_owned(),
            "beta".to_owned(),
            "alpha".to_owned(),
            "gamma".to_owned(),
            "beta".to_owned(),
        ];

        let unique = unique_ordered_refs(&values);

        assert_eq!(
            unique.into_iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["alpha", "beta", "gamma"]
        );
    }

    #[test]
    fn unique_ordered_refs_keeps_distinct_case_sensitive_values() {
        let values = vec!["Tag".to_owned(), "tag".to_owned(), "Tag".to_owned()];

        let unique = unique_ordered_refs(&values);

        assert_eq!(
            unique.into_iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["Tag", "tag"]
        );
    }
}
