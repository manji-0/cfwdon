use crate::{
    MastodonAccountResponse, Request, Response, Result, build_internal_cursor_link_header,
    parse_internal_pagination_id,
};

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct AccountCollectionQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
}

pub(crate) struct AccountCollectionPage {
    pub(crate) limit: u32,
    pub(crate) max_id: Option<i64>,
    pub(crate) since_id: Option<i64>,
}

impl AccountCollectionPage {
    pub(crate) fn from_query(
        query: &AccountCollectionQuery,
        default_limit: u32,
        max_limit: u32,
    ) -> Result<Self> {
        let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
        let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
        let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
        Ok(Self {
            limit: query.limit.unwrap_or(default_limit).clamp(1, max_limit),
            max_id,
            since_id: since_id.or(min_id),
        })
    }
}

#[derive(Debug)]
pub(crate) struct CollectionAccountEntry {
    pub(crate) cursor_id: i64,
    pub(crate) created_at: String,
    pub(crate) account: MastodonAccountResponse,
}

pub(crate) fn finalize_collection_response(
    req: &Request,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    mut entries: Vec<CollectionAccountEntry>,
) -> Result<Response> {
    entries.retain(|entry| max_id.is_none_or(|value| entry.cursor_id < value));
    entries.retain(|entry| since_id.is_none_or(|value| entry.cursor_id > value));
    entries.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.cursor_id.cmp(&left.cursor_id))
    });
    let has_next_page = entries.len() > limit as usize;
    if entries.len() > limit as usize {
        entries.truncate(limit as usize);
    }

    let first_id = entries.first().map(|entry| entry.cursor_id);
    let last_id = entries.last().map(|entry| entry.cursor_id);
    let response = entries
        .into_iter()
        .map(|entry| entry.account)
        .collect::<Vec<_>>();

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        req,
        limit,
        first_id,
        last_id,
        has_next_page,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_collection_page_clamps_limit_and_uses_min_id_as_since_id() {
        let query = AccountCollectionQuery {
            limit: Some(200),
            min_id: Some("42".to_owned()),
            ..Default::default()
        };

        let page = AccountCollectionPage::from_query(&query, 40, 80).unwrap();

        assert_eq!(page.limit, 80);
        assert_eq!(page.max_id, None);
        assert_eq!(page.since_id, Some(42));
    }

    #[test]
    fn account_collection_page_prefers_since_id_over_min_id() {
        let query = AccountCollectionQuery {
            limit: None,
            since_id: Some("99".to_owned()),
            min_id: Some("42".to_owned()),
            ..Default::default()
        };

        let page = AccountCollectionPage::from_query(&query, 20, 40).unwrap();

        assert_eq!(page.limit, 20);
        assert_eq!(page.max_id, None);
        assert_eq!(page.since_id, Some(99));
    }
}
