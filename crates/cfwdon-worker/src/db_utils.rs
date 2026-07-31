use super::{D1Database, Result};
use worker::d1::D1Type;

/// Cloudflare D1 (SQLite) rejects statements with more than 100 bound parameters.
/// Prefer [`sql_in_json_each`] for variable-length membership tests so bind count
/// stays O(1) instead of scaling with the ID list.
#[allow(dead_code)] // retained as the documented platform limit for fixed placeholder SQL
pub(crate) const D1_MAX_BOUND_PARAMETERS: usize = 100;

pub(crate) fn sql_placeholders(start: usize, len: usize) -> String {
    (start..start + len)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// SQL fragment `IN (SELECT value FROM json_each(?N))` — one bind for any list size.
pub(crate) fn sql_in_json_each(bind_index: usize) -> String {
    format!("IN (SELECT value FROM json_each(?{bind_index}))")
}

/// JSON text array bind payload for [`sql_in_json_each`].
pub(crate) fn json_string_array(values: &[impl AsRef<str>]) -> String {
    let list = values
        .iter()
        .map(|value| value.as_ref())
        .collect::<Vec<_>>();
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_owned())
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
    use super::{
        D1_MAX_BOUND_PARAMETERS, json_string_array, sql_in_json_each, sql_placeholders,
        unique_ordered_refs,
    };

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

    #[test]
    fn sql_in_json_each_uses_a_single_numbered_bind() {
        assert_eq!(sql_in_json_each(1), "IN (SELECT value FROM json_each(?1))");
        assert_eq!(sql_in_json_each(3), "IN (SELECT value FROM json_each(?3))");
    }

    #[test]
    fn json_string_array_encodes_values_as_json_text() {
        assert_eq!(json_string_array(&["a", "b"]), r#"["a","b"]"#);
        let empty: [&str; 0] = [];
        assert_eq!(json_string_array(&empty), "[]");
        assert_eq!(json_string_array(&["quote\"here"]), r#"["quote\"here"]"#);
    }

    #[test]
    fn sql_placeholders_can_fill_a_full_d1_bind_budget() {
        let placeholders = sql_placeholders(1, D1_MAX_BOUND_PARAMETERS);
        assert!(placeholders.starts_with("?1"));
        assert!(placeholders.ends_with("?100"));
        assert_eq!(placeholders.matches('?').count(), D1_MAX_BOUND_PARAMETERS);
    }
}
