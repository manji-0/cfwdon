use super::{
    FilterKeywordRow, FilterRow, FilterStatusRow, V1FilterRow, list_filter_keywords,
    list_filter_statuses,
};
use crate::Result;

pub(in crate::filters) fn split_filter_context(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(in crate::filters) fn keyword_document(row: &FilterKeywordRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "keyword": row.keyword,
        "whole_word": row.whole_word != 0,
    })
}

pub(in crate::filters) fn status_filter_document(row: &FilterStatusRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "status_id": row.status_id,
    })
}

pub(in crate::filters) fn filter_summary_document(row: &FilterRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "title": row.title,
        "context": split_filter_context(&row.context_csv),
        "expires_at": crate::timestamp_to_mastodon_iso8601_opt(row.expires_at.as_deref()),
        "filter_action": row.filter_action,
    })
}

pub(in crate::filters) fn v1_filter_document(row: &V1FilterRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "phrase": row.phrase,
        "context": split_filter_context(&row.context_csv),
        "expires_at": crate::timestamp_to_mastodon_iso8601_opt(row.expires_at.as_deref()),
        "irreversible": row.filter_action == "hide",
        "whole_word": row.whole_word != 0,
    })
}

pub(in crate::filters) fn v2_filter_document_from_parts(
    row: &FilterRow,
    keywords: &[FilterKeywordRow],
    statuses: &[FilterStatusRow],
) -> serde_json::Value {
    let keywords = keywords.iter().map(keyword_document).collect::<Vec<_>>();
    let statuses = statuses
        .iter()
        .map(status_filter_document)
        .collect::<Vec<_>>();

    serde_json::json!({
        "id": row.id,
        "title": row.title,
        "context": split_filter_context(&row.context_csv),
        "expires_at": crate::timestamp_to_mastodon_iso8601_opt(row.expires_at.as_deref()),
        "filter_action": row.filter_action,
        "keywords": keywords,
        "statuses": statuses,
    })
}

pub(in crate::filters) async fn v2_filter_document(
    db: &crate::D1Database,
    row: &FilterRow,
) -> Result<serde_json::Value> {
    let (keywords, statuses) = futures_util::try_join!(
        list_filter_keywords(db, &row.id),
        list_filter_statuses(db, &row.id),
    )?;

    Ok(v2_filter_document_from_parts(row, &keywords, &statuses))
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
    fn v2_filter_document_from_parts_embeds_preloaded_keywords_and_statuses() {
        let filter = FilterRow {
            id: "filter-1".to_owned(),
            title: "Quiet words".to_owned(),
            context_csv: "home, notifications".to_owned(),
            expires_at: None,
            filter_action: "warn".to_owned(),
        };
        let keywords = vec![FilterKeywordRow {
            id: "keyword-1".to_owned(),
            filter_id: "filter-1".to_owned(),
            keyword: "launch".to_owned(),
            whole_word: 1,
        }];
        let statuses = vec![FilterStatusRow {
            id: "status-filter-1".to_owned(),
            filter_id: "filter-1".to_owned(),
            status_id: "status-1".to_owned(),
        }];

        let document = v2_filter_document_from_parts(&filter, &keywords, &statuses);

        assert_eq!(document["id"], serde_json::json!("filter-1"));
        assert_eq!(
            document["context"],
            serde_json::json!(["home", "notifications"])
        );
        assert_eq!(
            document["keywords"][0]["keyword"],
            serde_json::json!("launch")
        );
        assert_eq!(
            document["statuses"][0]["status_id"],
            serde_json::json!("status-1")
        );
    }
}
