use super::{D1Database, Result};
use worker::d1::D1Type;

/// Cloudflare D1 (SQLite) rejects statements with more than 100 bound parameters.
pub(crate) const D1_MAX_BOUND_PARAMETERS: usize = 100;

pub(crate) fn sql_placeholders(start: usize, len: usize) -> String {
    (start..start + len)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Max values for one `IN (...)` list when `leading_bound_parameters` are already reserved.
pub(crate) fn d1_in_value_chunk_size(leading_bound_parameters: usize) -> usize {
    D1_MAX_BOUND_PARAMETERS
        .saturating_sub(leading_bound_parameters)
        .max(1)
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
        D1_MAX_BOUND_PARAMETERS, d1_in_value_chunk_size, sql_placeholders, unique_ordered_refs,
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
    fn d1_in_value_chunk_size_respects_leading_binds() {
        assert_eq!(d1_in_value_chunk_size(0), D1_MAX_BOUND_PARAMETERS);
        assert_eq!(d1_in_value_chunk_size(1), D1_MAX_BOUND_PARAMETERS - 1);
        assert_eq!(d1_in_value_chunk_size(D1_MAX_BOUND_PARAMETERS), 1);
        assert_eq!(d1_in_value_chunk_size(D1_MAX_BOUND_PARAMETERS + 5), 1);
    }

    #[test]
    fn sql_placeholders_can_fill_a_full_d1_bind_budget() {
        let placeholders = sql_placeholders(1, D1_MAX_BOUND_PARAMETERS);
        assert!(placeholders.starts_with("?1"));
        assert!(placeholders.ends_with("?100"));
        assert_eq!(placeholders.matches('?').count(), D1_MAX_BOUND_PARAMETERS);
    }
}
