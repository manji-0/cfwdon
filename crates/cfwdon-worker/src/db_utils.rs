use super::{D1Database, Result};
use serde::de::DeserializeOwned;
use worker::Error;
use worker::d1::{D1Result, D1Type};

/// Cloudflare D1 (SQLite) rejects statements with more than 100 bound parameters.
/// Prefer [`sql_in_json_each`] for variable-length membership tests so bind count
/// stays O(1) instead of scaling with the ID list.
#[allow(dead_code)] // retained as the documented platform limit for fixed placeholder SQL
pub(crate) const D1_MAX_BOUND_PARAMETERS: usize = 100;

/// Decode D1 `all()` rows without panicking on column/type mismatches.
///
/// `worker::d1::D1Result::results` uses `unwrap()` on `serde_wasm_bindgen::from_value`,
/// which turns a missing SQL column into an opaque Workers hang/500. Route through
/// `serde_json::Value` first so callers get a structured `Result` instead.
pub(crate) fn d1_results<T>(result: &D1Result) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let rows = result.results::<serde_json::Value>()?;
    deserialize_d1_row_values(rows)
}

pub(crate) fn deserialize_d1_row_values<T>(rows: Vec<serde_json::Value>) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let type_name = std::any::type_name::<T>();
    rows.into_iter()
        .enumerate()
        .map(|(index, value)| {
            serde_json::from_value(value).map_err(|error| {
                Error::RustError(format!(
                    "D1 row deserialize failed at index {index} into {type_name}: {error}"
                ))
            })
        })
        .collect()
}

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
        D1_MAX_BOUND_PARAMETERS, deserialize_d1_row_values, json_string_array, sql_in_json_each,
        sql_placeholders, unique_ordered_refs,
    };
    use cfwdon_domain::LocalAccountRecord;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct TinyRow {
        id: String,
        locked: i32,
    }

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

    #[test]
    fn deserialize_d1_row_values_returns_structured_error_on_missing_field() {
        let rows = vec![serde_json::json!({ "id": "acct-1" })];
        let error =
            deserialize_d1_row_values::<TinyRow>(rows).expect_err("missing locked must fail");
        let message = error.to_string();
        assert!(
            message.contains("D1 row deserialize failed at index 0"),
            "{message}"
        );
        assert!(
            message.contains("TinyRow") || message.contains("locked"),
            "{message}"
        );
    }

    fn account_row_fixture_json() -> serde_json::Value {
        let record = LocalAccountRecord::test_fixture("acct-1", "alice");
        serde_json::json!({
            "id": record.id,
            "username": record.username,
            "access_email": record.access_email,
            "display_name": record.display_name,
            "bio_html": record.bio_html,
            "bio_text": record.bio_text,
            "fields_json": record.fields_json,
            "locked": record.locked,
            "bot": record.bot,
            "discoverable": record.discoverable,
            "default_post_visibility": record.default_post_visibility,
            "default_quote_policy": record.default_quote_policy,
            "default_sensitive": record.default_sensitive,
            "default_language": record.default_language,
            "avatar_object_key": record.avatar_object_key,
            "avatar_content_type": record.avatar_content_type,
            "header_object_key": record.header_object_key,
            "header_content_type": record.header_content_type,
            "private_key_jwk": record.private_key_jwk,
            "public_key_pem": record.public_key_pem,
            "created_at": record.created_at,
        })
    }

    #[test]
    fn local_account_record_deserialize_rejects_missing_locked_or_bot() {
        let mut row = account_row_fixture_json();
        row.as_object_mut().expect("object").remove("locked");
        let error = deserialize_d1_row_values::<LocalAccountRecord>(vec![row])
            .expect_err("missing locked must fail");
        assert!(
            error.to_string().contains("D1 row deserialize failed"),
            "{}",
            error
        );

        let mut row = account_row_fixture_json();
        row.as_object_mut().expect("object").remove("bot");
        let error = deserialize_d1_row_values::<LocalAccountRecord>(vec![row])
            .expect_err("missing bot must fail");
        assert!(
            error.to_string().contains("D1 row deserialize failed"),
            "{}",
            error
        );
    }

    #[test]
    fn local_account_record_fixture_roundtrips_via_d1_row_helper() {
        let row = account_row_fixture_json();
        let decoded = deserialize_d1_row_values::<LocalAccountRecord>(vec![row])
            .expect("fixture must deserialize");
        assert_eq!(decoded[0].username, "alice");
        assert_eq!(decoded[0].locked, 0);
        assert_eq!(decoded[0].bot, 0);
    }
}
