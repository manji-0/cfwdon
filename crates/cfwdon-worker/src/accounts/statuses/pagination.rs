use crate::{AccountStatusesQuery, Request, Result};

#[derive(Debug, PartialEq)]
pub(crate) struct AccountStatusesRequestOptions<'a> {
    pub(crate) limit: u32,
    pub(crate) query_limit: u32,
    pub(crate) wants_html: bool,
    pub(crate) min_id: Option<&'a str>,
}

pub(crate) fn account_statuses_request_options<'a>(
    query: &'a AccountStatusesQuery,
    accept: &str,
) -> AccountStatusesRequestOptions<'a> {
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    AccountStatusesRequestOptions {
        limit,
        query_limit: crate::timeline_fetch_limit(limit),
        wants_html: accept_prefers_statuses_html(accept),
        min_id: query.min_id.as_deref().or(query.since_id.as_deref()),
    }
}

fn accept_prefers_statuses_html(accept: &str) -> bool {
    let accept = accept.to_ascii_lowercase();
    accept.contains("text/html") && !accept.contains("application/json")
}

pub(crate) fn account_statuses_older_page_url(
    req: &Request,
    limit: u32,
    max_id: &str,
) -> Result<String> {
    let mut url = req.url()?;
    let preserved_params = url
        .query_pairs()
        .filter(|(key, _)| {
            key != "max_id" && key != "since_id" && key != "min_id" && key != "limit"
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (key, value) in preserved_params {
            query.append_pair(&key, &value);
        }
        query.append_pair("limit", &limit.to_string());
        query.append_pair("max_id", max_id);
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_statuses_request_options_default_and_clamped_limits() {
        let query = AccountStatusesQuery::default();
        let options = account_statuses_request_options(&query, "");
        assert_eq!(options.limit, 20);
        assert_eq!(options.query_limit, crate::timeline_fetch_limit(20));
        assert!(!options.wants_html);
        assert_eq!(options.min_id, None);

        let query = AccountStatusesQuery {
            limit: Some(0),
            ..Default::default()
        };
        assert_eq!(account_statuses_request_options(&query, "").limit, 1);

        let query = AccountStatusesQuery {
            limit: Some(200),
            ..Default::default()
        };
        assert_eq!(account_statuses_request_options(&query, "").limit, 40);
    }

    #[test]
    fn account_statuses_request_options_prefers_min_id_over_since_id() {
        let query = AccountStatusesQuery {
            min_id: Some("min".to_owned()),
            since_id: Some("since".to_owned()),
            ..Default::default()
        };
        let options = account_statuses_request_options(&query, "");
        assert_eq!(options.min_id, Some("min"));

        let query = AccountStatusesQuery {
            since_id: Some("since".to_owned()),
            ..Default::default()
        };
        let options = account_statuses_request_options(&query, "");
        assert_eq!(options.min_id, Some("since"));
    }

    #[test]
    fn accept_prefers_statuses_html_only_without_json_preference() {
        assert!(accept_prefers_statuses_html("text/html"));
        assert!(accept_prefers_statuses_html("TEXT/HTML; charset=utf-8"));
        assert!(!accept_prefers_statuses_html("application/json"));
        assert!(!accept_prefers_statuses_html(
            "text/html, application/json;q=0.9"
        ));
    }
}
