use super::bindings::{
    remote_status_id_bindings, remote_status_lookup_value_bindings,
    remote_status_object_uri_bindings,
};
use super::records::{RemoteStatusRecord, RemoteStatusRow, remote_status_from_record};
use crate::D1Database;
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{Error, Result};

pub(super) const REMOTE_STATUS_ROW_SELECT: &str = "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, published_at, edited_at, card_json, federated_emojis_json, in_reply_to_id, COALESCE(rsc.favourites_count, 0) AS favourites_count, COALESCE(rsc.reblogs_count, 0) AS reblogs_count
         FROM remote_statuses
         LEFT JOIN remote_status_counts rsc ON rsc.remote_status_id = remote_statuses.id";

pub(super) fn remote_statuses_by_url_or_object_uris_sql() -> String {
    let in_list = crate::sql_in_json_each(1);
    format!(
        "{REMOTE_STATUS_ROW_SELECT}
         WHERE object_uri {in_list}
         UNION
         {REMOTE_STATUS_ROW_SELECT}
         WHERE url {in_list}"
    )
}

pub(crate) async fn find_remote_status_by_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<RemoteStatusRow>> {
    let bindings = remote_status_id_bindings(status_id);
    db.prepare(format!(
        "{REMOTE_STATUS_ROW_SELECT}
         WHERE id = ?1
         LIMIT 1"
    ))
    .bind_refs(bindings.iter())?
    .first::<RemoteStatusRecord>(None)
    .await
    .and_then(|row| row.map(remote_status_from_record).transpose())
}

pub(crate) async fn find_remote_status_raw_object_by_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<serde_json::Value>> {
    #[derive(Deserialize)]
    struct RemoteStatusRawObjectRow {
        raw_object_json: String,
    }

    let bindings = remote_status_id_bindings(status_id);
    let Some(row) = db
        .prepare(
            "SELECT raw_object_json
             FROM remote_statuses
             WHERE id = ?1
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<RemoteStatusRawObjectRow>(None)
        .await?
    else {
        return Ok(None);
    };

    serde_json::from_str(&row.raw_object_json)
        .map(Some)
        .map_err(|error| Error::RustError(format!("failed to parse remote status object: {error}")))
}

pub(crate) async fn find_remote_status_by_object_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Option<RemoteStatusRow>> {
    let bindings = remote_status_object_uri_bindings(object_uri);
    db.prepare(format!(
        "{REMOTE_STATUS_ROW_SELECT}
         WHERE object_uri = ?1
         LIMIT 1"
    ))
    .bind_refs(bindings.iter())?
    .first::<RemoteStatusRecord>(None)
    .await
    .and_then(|row| row.map(remote_status_from_record).transpose())
}

pub(crate) async fn find_remote_status_by_url_or_object_uri(
    db: &D1Database,
    value: &str,
) -> Result<Option<RemoteStatusRow>> {
    if let Some(row) = find_remote_status_by_object_uri(db, value).await? {
        return Ok(Some(row));
    }
    let bindings = remote_status_lookup_value_bindings(value);
    db.prepare(format!(
        "{REMOTE_STATUS_ROW_SELECT}
         WHERE url = ?1
         LIMIT 1"
    ))
    .bind_refs(bindings.iter())?
    .first::<RemoteStatusRecord>(None)
    .await
    .and_then(|row| row.map(remote_status_from_record).transpose())
}

/// Batch form of [`find_remote_status_by_url_or_object_uri`].
///
/// Matches the single-value lookup: a row qualifies when either its
/// `object_uri` or its `url` is in `values`. Callers resolve which requested
/// value produced a row by checking both columns.
pub(crate) async fn find_remote_statuses_by_url_or_object_uris(
    db: &D1Database,
    values: &[String],
) -> Result<Vec<RemoteStatusRow>> {
    let values = crate::unique_ordered_refs(values);
    if values.is_empty() {
        return Ok(Vec::new());
    }

    let values_json = crate::json_string_array(&values);
    let sql = remote_statuses_by_url_or_object_uris_sql();
    let binding = D1Type::Text(values_json.as_str());
    let result = db.prepare(sql).bind_refs(&binding)?.all().await?;

    crate::d1_results::<RemoteStatusRecord>(&result)?
        .into_iter()
        .map(remote_status_from_record)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_statuses_by_url_or_object_uris_sql_uses_union_not_or() {
        let sql = remote_statuses_by_url_or_object_uris_sql();
        assert!(sql.contains("UNION"));
        assert!(!sql.contains(" OR "));
        assert!(sql.contains("WHERE object_uri IN (SELECT value FROM json_each(?1))"));
        assert!(sql.contains("WHERE url IN (SELECT value FROM json_each(?1))"));
    }
}
