use serde::Deserialize;
use url::Url;

const SEARCH_QUOTE_EQUIVALENT_CHARACTERS: [char; 11] =
    ['“', '”', '„', '«', '»', '「', '」', '『', '』', '《', '》'];

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

fn normalized_search_v2_type(search_type: Option<&str>) -> Option<String> {
    search_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

pub(crate) fn search_category_flags(search_type: Option<&str>) -> SearchCategoryFlags {
    match normalized_search_v2_type(search_type).as_deref() {
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

pub(crate) fn search_v2_type_allows_url_resource(
    search_type: Option<&str>,
    resource_kind: &str,
) -> bool {
    match normalized_search_v2_type(search_type).as_deref() {
        None => true,
        Some("accounts") => resource_kind == "accounts",
        Some("statuses") => resource_kind == "statuses",
        Some("hashtags") => resource_kind == "hashtags",
        Some(_) => false,
    }
}

pub(crate) fn search_v2_requires_auth(query: &SearchV2Query) -> bool {
    query.resolve.unwrap_or(false) || effective_search_v2_offset(query) > 0
}

pub(crate) fn search_v2_unauthenticated_error(query: &SearchV2Query) -> Option<&'static str> {
    if effective_search_v2_offset(query) > 0 {
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

pub(crate) fn effective_search_v2_offset(query: &SearchV2Query) -> u32 {
    if normalized_search_v2_type(query.search_type.as_deref()).is_some() {
        query.offset.unwrap_or(0)
    } else {
        0
    }
}

pub(crate) fn effective_search_v2_following(query: &SearchV2Query, authenticated: bool) -> bool {
    authenticated && query.following.unwrap_or(false)
}

pub(crate) fn normalize_search_query_input(query: &str) -> String {
    query
        .chars()
        .map(|ch| {
            if SEARCH_QUOTE_EQUIVALENT_CHARACTERS.contains(&ch) {
                '"'
            } else {
                ch
            }
        })
        .collect()
}

fn fold_search_match_character(ch: char) -> Option<&'static str> {
    match ch {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ǎ' => Some("a"),
        'æ' => Some("ae"),
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => Some("c"),
        'ď' | 'đ' => Some("d"),
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => Some("e"),
        'ƒ' => Some("f"),
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => Some("g"),
        'ĥ' | 'ħ' => Some("h"),
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => Some("i"),
        'ĵ' => Some("j"),
        'ķ' => Some("k"),
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => Some("l"),
        'ñ' | 'ń' | 'ņ' | 'ň' => Some("n"),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => Some("o"),
        'œ' => Some("oe"),
        'ŕ' | 'ŗ' | 'ř' => Some("r"),
        'ś' | 'ŝ' | 'ş' | 'š' => Some("s"),
        'ß' => Some("ss"),
        'ţ' | 'ť' | 'ŧ' => Some("t"),
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => Some("u"),
        'ŵ' => Some("w"),
        'ý' | 'ÿ' | 'ŷ' => Some("y"),
        'ź' | 'ż' | 'ž' => Some("z"),
        _ => None,
    }
}

pub(crate) fn normalize_search_match_text(value: &str) -> String {
    let normalized = normalize_search_query_input(value);
    let mut folded = String::new();

    for ch in normalized.trim().chars() {
        for lowercase in ch.to_lowercase() {
            if let Some(value) = fold_search_match_character(lowercase) {
                folded.push_str(value);
            } else {
                folded.push(lowercase);
            }
        }
    }

    folded
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
    let query = normalize_search_match_text(query);
    if query.is_empty() {
        return 3;
    }

    let candidate = normalize_search_match_text(candidate);
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
