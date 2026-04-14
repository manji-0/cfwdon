use super::{Error, FormData, FormEntry, Request, Result, RouteContext};
use url::Url;

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
    build_internal_cursor_link_for_url(&req.url()?, limit, max_id, since_id, rel)
}

pub(crate) fn build_internal_cursor_link_for_url(
    url: &Url,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
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
    }

    Ok(format!("<{}>; rel=\"{}\"", url, rel))
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

pub(crate) fn parse_internal_pagination_id(
    value: Option<&str>,
    field: &str,
) -> Result<Option<i64>> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => value
            .parse::<i64>()
            .map(Some)
            .map_err(|_| Error::RustError(format!("{field} must be an integer cursor id"))),
    }
}

pub(crate) fn parse_media_ids_from_form(form: &FormData) -> Option<Vec<String>> {
    form.get_all("media_ids[]").map(|entries| {
        entries
            .into_iter()
            .filter_map(|entry| match entry {
                FormEntry::Field(value) => Some(value),
                FormEntry::File(_) => None,
            })
            .collect()
    })
}
