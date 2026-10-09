use url::Url;
use worker::{Error, FormData, FormEntry, Request, Response, Result, RouteContext};

pub(crate) fn build_internal_cursor_link_header(
    req: &Request,
    limit: u32,
    first_cursor: Option<i64>,
    last_cursor: Option<i64>,
    has_next: bool,
    has_prev: bool,
) -> Result<Option<String>> {
    let mut links = Vec::new();

    if has_next && let Some(cursor) = last_cursor {
        links.push(build_internal_cursor_link(
            req,
            limit,
            Some(cursor),
            None,
            "next",
        )?);
    }

    if has_prev && let Some(cursor) = first_cursor {
        links.push(build_internal_cursor_link(
            req,
            limit,
            None,
            Some(cursor),
            "prev",
        )?);
    }

    if links.is_empty() {
        return Ok(None);
    }

    Ok(Some(links.join(", ")))
}

pub(crate) fn build_internal_cursor_link(
    req: &Request,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    rel: &str,
) -> Result<String> {
    build_internal_cursor_link_for_url_with_min_id(&req.url()?, limit, max_id, since_id, None, rel)
}

pub(crate) fn build_internal_cursor_link_for_url(
    url: &Url,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    rel: &str,
) -> Result<String> {
    build_internal_cursor_link_for_url_with_min_id(url, limit, max_id, since_id, None, rel)
}

pub(crate) fn build_internal_cursor_link_for_url_with_min_id(
    url: &Url,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    min_id: Option<i64>,
    rel: &str,
) -> Result<String> {
    let mut url = url.clone();
    let pairs = url
        .query_pairs()
        .filter(|(key, _)| {
            key != "max_id" && key != "since_id" && key != "min_id" && key != "limit"
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (key, value) in pairs {
            query.append_pair(&key, &value);
        }
        query.append_pair("limit", &limit.to_string());
        if let Some(value) = max_id {
            query.append_pair("max_id", &value.to_string());
        }
        if let Some(value) = since_id {
            query.append_pair("since_id", &value.to_string());
        }
        if let Some(value) = min_id {
            query.append_pair("min_id", &value.to_string());
        }
    }

    Ok(format!("<{}>; rel=\"{}\"", url, rel))
}

/// Values of a Rails-style array query parameter: `key[]=a&key[]=b`, or a bare
/// repeated `key=a&key=b`. `serde_urlencoded` cannot fill a `Vec` field, and a
/// failed `Request::query` would drop every other parameter with it, so array
/// fields are `#[serde(skip)]` and filled from here.
pub(crate) fn query_array_param(url: &Url, key: &str) -> Option<Vec<String>> {
    let bracketed = format!("{key}[]");
    let values = url
        .query_pairs()
        .filter(|(name, _)| name == key || *name == bracketed)
        .map(|(_, value)| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

/// Query booleans the way Rails casts them: blank is absent, `0` / `f` /
/// `false` / `off` are false, anything else is true. Plain `serde_urlencoded`
/// only accepts `true` / `false` and rejects the whole query otherwise.
pub(crate) fn deserialize_query_bool<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = <Option<String> as serde::Deserialize>::deserialize(deserializer)?;
    Ok(value.and_then(|value| rails_query_bool(&value)))
}

fn rails_query_bool(value: &str) -> Option<bool> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some(!matches!(
        value.to_ascii_lowercase().as_str(),
        "0" | "f" | "false" | "off"
    ))
}

/// Offset-paged `Link` header as Mastodon's trends endpoints send it: `next`
/// while a page comes back full, `prev` once past the first page.
pub(crate) fn offset_link_header(
    url: &Url,
    limit: u32,
    offset: u32,
    page_len: usize,
) -> Option<String> {
    let link = |offset: u32, rel: &str| {
        let mut url = url.clone();
        let pairs = url
            .query_pairs()
            .filter(|(key, _)| key != "offset" && key != "limit")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        {
            let mut query = url.query_pairs_mut();
            query.clear();
            for (key, value) in pairs {
                query.append_pair(&key, &value);
            }
            query.append_pair("limit", &limit.to_string());
            query.append_pair("offset", &offset.to_string());
        }
        format!("<{url}>; rel=\"{rel}\"")
    };
    let mut links = Vec::new();
    if page_len >= limit as usize {
        links.push(link(offset.saturating_add(limit), "next"));
    }
    if offset > 0 {
        links.push(link(offset.saturating_sub(limit), "prev"));
    }
    (!links.is_empty()).then(|| links.join(", "))
}

pub(crate) fn parse_optional_bool(
    value: Option<&str>,
) -> std::result::Result<Option<bool>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "on" => Ok(Some(true)),
        "false" | "0" | "off" => Ok(Some(false)),
        _ => Err(format!("invalid boolean value: {value}")),
    }
}

pub(crate) fn status_id_from_context(ctx: &RouteContext<()>) -> Result<String> {
    ctx.param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing status id route parameter".to_owned()))
}

/// Mastodon's `max_id` / `since_id` / `min_id` paging cursors over an internal
/// integer id, cast the way Rails casts a query value for an integer column.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InternalPaginationIds {
    pub(crate) max_id: Option<i64>,
    pub(crate) since_id: Option<i64>,
    pub(crate) min_id: Option<i64>,
}

impl InternalPaginationIds {
    /// `None` when a cursor is not numeric (an account id, say). Rails casts it
    /// to NULL, the comparison matches no row, and Mastodon answers with an
    /// empty page rather than an error; callers do the same with
    /// [`empty_page_response`].
    pub(crate) fn parse(
        max_id: Option<&str>,
        since_id: Option<&str>,
        min_id: Option<&str>,
    ) -> Option<Self> {
        Some(Self {
            max_id: cast_pagination_id(max_id)?,
            since_id: cast_pagination_id(since_id)?,
            min_id: cast_pagination_id(min_id)?,
        })
    }
}

/// Rails' integer cast: blank is absent, a value opening with an optional sign
/// and a digit reads up to its first non-digit (`"42abc"` is 42), and anything
/// else, or a value past the column range, matches nothing (outer `None`).
fn cast_pagination_id(value: Option<&str>) -> Option<Option<i64>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Some(None);
    };
    let unsigned = value.trim_start_matches(['+', '-']);
    if value.len() - unsigned.len() > 1 {
        return None;
    }
    let digits = unsigned
        .find(|ch: char| !ch.is_ascii_digit())
        .map_or(unsigned, |end| &unsigned[..end]);
    if digits.is_empty() {
        return None;
    }
    let sign_len = value.len() - unsigned.len();
    value[..sign_len + digits.len()]
        .parse::<i64>()
        .ok()
        .map(Some)
}

/// The empty page Mastodon returns when a paging cursor matches no row.
pub(crate) fn empty_page_response() -> Result<Response> {
    Response::from_json(&Vec::<serde_json::Value>::new())
}

pub(crate) fn parse_media_ids_from_form(form: &FormData) -> Option<Vec<String>> {
    parse_media_id_fields([
        form.get_all("media_ids[]"),
        form.get_all("media_ids"),
        form.get_all("media_ids[0]"),
        form.get_all("media_ids[1]"),
        form.get_all("media_ids[2]"),
        form.get_all("media_ids[3]"),
    ])
}

pub(crate) fn parse_media_id_fields<const N: usize>(
    fields: [Option<Vec<FormEntry>>; N],
) -> Option<Vec<String>> {
    let media_ids = fields
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| match entry {
            FormEntry::Field(value) => Some(value),
            FormEntry::File(_) => None,
        })
        .collect::<Vec<_>>();

    if media_ids.is_empty() {
        None
    } else {
        Some(media_ids)
    }
}

#[cfg(test)]
mod unit_tests;

#[cfg(test)]
mod query_param_tests {
    use super::{offset_link_header, query_array_param, rails_query_bool};
    use serde::Deserialize;

    #[derive(Debug, Default, Deserialize)]
    struct ProbeQuery {
        limit: Option<u32>,
        #[serde(default, deserialize_with = "super::deserialize_query_bool")]
        only_media: Option<bool>,
        #[serde(skip)]
        types: Option<Vec<String>>,
    }

    fn parse(query: &str) -> ProbeQuery {
        let url = url::Url::parse(&format!("https://example.com/?{query}")).unwrap();
        let mut parsed: ProbeQuery =
            ProbeQuery::deserialize(serde_urlencoded::Deserializer::new(url.query_pairs()))
                .unwrap();
        parsed.types = query_array_param(&url, "types");
        parsed
    }

    #[test]
    fn array_params_do_not_reset_the_rest_of_the_query() {
        let query = parse("limit=5&types[]=mention&types[]=favourite");
        assert_eq!(query.limit, Some(5));
        assert_eq!(
            query.types,
            Some(vec!["mention".to_owned(), "favourite".to_owned()])
        );
        assert_eq!(parse("types=follow").types, Some(vec!["follow".to_owned()]));
        assert_eq!(parse("limit=1").types, None);
    }

    #[test]
    fn query_bools_follow_rails_casting() {
        assert_eq!(parse("only_media=1").only_media, Some(true));
        assert_eq!(parse("only_media=true").only_media, Some(true));
        assert_eq!(parse("only_media=0").only_media, Some(false));
        assert_eq!(parse("only_media=off").only_media, Some(false));
        assert_eq!(parse("only_media=").only_media, None);
        assert_eq!(parse("limit=2").only_media, None);
        assert_eq!(rails_query_bool("F"), Some(false));
    }

    #[test]
    fn offset_link_header_pages_by_offset() {
        let url = url::Url::parse("https://example.com/api/v1/trends/statuses?offset=20").unwrap();
        assert_eq!(
            offset_link_header(&url, 20, 20, 20).as_deref(),
            Some(
                "<https://example.com/api/v1/trends/statuses?limit=20&offset=40>; rel=\"next\", \
                 <https://example.com/api/v1/trends/statuses?limit=20&offset=0>; rel=\"prev\""
            )
        );
        assert_eq!(offset_link_header(&url, 20, 0, 5), None);
    }
}
