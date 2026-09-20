use super::{
    AccountFilterMatcher, filter_summary_document, list_filter_keywords_for_filters,
    list_filter_statuses_for_filters, list_filters,
};
use crate::Result;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

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
        let Ok(datetime) = OffsetDateTime::parse(value, &Rfc3339) else {
            return false;
        };
        let Ok(now) = OffsetDateTime::from_unix_timestamp(crate::now_unix_timestamp()) else {
            return false;
        };
        datetime <= now
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
    db: &crate::D1Database,
    account_id: &str,
) -> Result<AccountFilterMatcher> {
    if !crate::load_account_capabilities(db, account_id)
        .await?
        .has_filters
    {
        return Ok(AccountFilterMatcher::default());
    }

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
    db: &crate::D1Database,
    account_id: &str,
    status_id: &str,
    text: &str,
    spoiler_text: &str,
) -> Result<Vec<serde_json::Value>> {
    Ok(load_account_filter_matcher(db, account_id)
        .await?
        .filtered_status(status_id, text, spoiler_text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::{FilterKeywordRow, FilterRow, FilterStatusRow};
    use std::collections::HashMap;

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
