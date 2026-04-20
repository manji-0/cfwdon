use serde::Deserialize;
use url::Url;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchCategoryFlags {
    pub(crate) accounts: bool,
    pub(crate) statuses: bool,
    pub(crate) hashtags: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchUrlQueryMode {
    None,
    ResolveOnly,
    EmptyResults,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct SearchV2Query {
    pub(crate) q: String,
    #[serde(rename = "type")]
    pub(crate) search_type: Option<String>,
    pub(crate) resolve: Option<bool>,
    pub(crate) following: Option<bool>,
    pub(crate) account_id: Option<String>,
    #[serde(rename = "exclude_unreviewed")]
    pub(crate) _exclude_unreviewed: Option<bool>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
    pub(crate) limit: Option<u32>,
    pub(crate) offset: Option<u32>,
}

pub(crate) fn search_category_flags(search_type: Option<&str>) -> SearchCategoryFlags {
    match search_type.map(str::trim).filter(|value| !value.is_empty()) {
        None => SearchCategoryFlags {
            accounts: true,
            statuses: true,
            hashtags: true,
        },
        Some("accounts") => SearchCategoryFlags {
            accounts: true,
            statuses: false,
            hashtags: false,
        },
        Some("statuses") => SearchCategoryFlags {
            accounts: false,
            statuses: true,
            hashtags: false,
        },
        Some("hashtags") => SearchCategoryFlags {
            accounts: false,
            statuses: false,
            hashtags: true,
        },
        Some(_) => SearchCategoryFlags::default(),
    }
}

pub(crate) fn search_v2_requires_auth(query: &SearchV2Query) -> bool {
    query.resolve.unwrap_or(false) || query.offset.unwrap_or(0) > 0
}

pub(crate) fn search_v2_unauthenticated_error(query: &SearchV2Query) -> Option<&'static str> {
    if query.offset.unwrap_or(0) > 0 {
        Some("Search queries pagination is not supported without authentication")
    } else if query.resolve.unwrap_or(false) {
        Some(
            "Search queries that resolve remote resources are not supported without authentication",
        )
    } else {
        None
    }
}

pub(crate) fn search_v2_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(20).clamp(1, 40)
}

pub(crate) fn search_v2_url_query_mode(
    query: &str,
    resolve_enabled: bool,
    offset: u32,
) -> SearchUrlQueryMode {
    if !resolve_enabled {
        return SearchUrlQueryMode::None;
    }

    let Ok(url) = Url::parse(query.trim()) else {
        return SearchUrlQueryMode::None;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return SearchUrlQueryMode::None;
    }

    if offset > 0 {
        SearchUrlQueryMode::EmptyResults
    } else {
        SearchUrlQueryMode::ResolveOnly
    }
}

pub(crate) fn search_text_match_rank(query: &str, candidate: &str) -> u8 {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return 3;
    }

    let candidate = candidate.trim().to_ascii_lowercase();
    if candidate == query {
        0
    } else if candidate.starts_with(&query) {
        1
    } else if candidate.contains(&query) {
        2
    } else {
        3
    }
}
