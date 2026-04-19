use super::normalize_hashtag;
use serde::Deserialize;
use std::collections::HashSet;
use worker::{D1Database, Request, Result};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TimelinePaginationQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicTimelineQuery {
    #[serde(flatten)]
    pub(crate) pagination: TimelinePaginationQuery,
    pub(crate) local: Option<bool>,
    pub(crate) remote: Option<bool>,
    pub(crate) only_media: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TagTimelineQuery {
    #[serde(flatten)]
    pub(crate) pagination: TimelinePaginationQuery,
    pub(crate) only_media: Option<bool>,
    pub(crate) local: Option<bool>,
    pub(crate) remote: Option<bool>,
    #[serde(rename = "any[]")]
    pub(crate) any: Option<Vec<String>>,
    #[serde(rename = "all[]")]
    pub(crate) all: Option<Vec<String>>,
    #[serde(rename = "none[]")]
    pub(crate) none: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct HomeTimelineQuery {
    #[serde(flatten)]
    pub(crate) pagination: TimelinePaginationQuery,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct LinkTimelineQuery {
    #[serde(flatten)]
    pub(crate) pagination: TimelinePaginationQuery,
    pub(crate) url: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedTimelineCursor {
    pub(crate) max_id: Option<String>,
    pub(crate) max_timestamp: Option<String>,
    pub(crate) min_id: Option<String>,
    pub(crate) min_timestamp: Option<String>,
}

pub(crate) fn timeline_limit(pagination: &TimelinePaginationQuery) -> u32 {
    pagination.limit.unwrap_or(20).clamp(1, 40)
}

pub(crate) fn timeline_fetch_limit(limit: u32) -> u32 {
    limit.saturating_mul(4).clamp(limit, 160)
}

pub(crate) async fn resolve_timeline_cursor(
    db: &D1Database,
    pagination: &TimelinePaginationQuery,
) -> Result<ResolvedTimelineCursor> {
    let max_id = normalize_timeline_cursor(pagination.max_id.as_deref());
    let min_id = normalize_timeline_cursor(
        pagination
            .min_id
            .as_deref()
            .or(pagination.since_id.as_deref()),
    );

    Ok(ResolvedTimelineCursor {
        max_timestamp: resolve_timeline_cursor_timestamp(db, max_id.as_deref()).await?,
        min_timestamp: resolve_timeline_cursor_timestamp(db, min_id.as_deref()).await?,
        max_id,
        min_id,
    })
}

pub(crate) fn build_timeline_link_header(
    req: &Request,
    limit: u32,
    first_id: Option<&str>,
    last_id: Option<&str>,
) -> Result<Option<String>> {
    Ok(build_timeline_link_header_for_url(
        &req.url()?,
        limit,
        first_id,
        last_id,
    ))
}

pub(crate) fn build_timeline_link_header_for_url(
    url: &url::Url,
    limit: u32,
    first_id: Option<&str>,
    last_id: Option<&str>,
) -> Option<String> {
    let preserved_params = url
        .query_pairs()
        .filter(|(key, _)| {
            key != "max_id" && key != "since_id" && key != "min_id" && key != "limit"
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut links = Vec::new();

    if let Some(last_id) = last_id.filter(|value| !value.is_empty()) {
        links.push(build_timeline_link(
            url,
            limit,
            &preserved_params,
            Some(("max_id", last_id)),
            "next",
        ));
    }
    if let Some(first_id) = first_id.filter(|value| !value.is_empty()) {
        links.push(build_timeline_link(
            url,
            limit,
            &preserved_params,
            Some(("min_id", first_id)),
            "prev",
        ));
    }

    (!links.is_empty()).then(|| links.join(", "))
}

fn normalize_timeline_cursor(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

async fn resolve_timeline_cursor_timestamp(
    db: &D1Database,
    cursor_id: Option<&str>,
) -> Result<Option<String>> {
    let Some(cursor_id) = cursor_id else {
        return Ok(None);
    };

    if let Some(status) = crate::find_status_by_id(db, cursor_id).await? {
        return Ok(Some(status.created_at));
    }
    if let Some(status) = crate::find_remote_status_by_id(db, cursor_id).await? {
        return Ok(Some(status.published_at));
    }

    Ok(None)
}

fn build_timeline_link(
    base_url: &url::Url,
    limit: u32,
    preserved_params: &[(String, String)],
    cursor: Option<(&str, &str)>,
    rel: &str,
) -> String {
    let mut url = base_url.clone();
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (key, value) in preserved_params {
            query.append_pair(key, value);
        }
        query.append_pair("limit", &limit.to_string());
        if let Some((key, value)) = cursor {
            query.append_pair(key, value);
        }
    }

    format!("<{}>; rel=\"{}\"", url, rel)
}

pub(crate) fn include_local_source(local: Option<bool>, remote: Option<bool>) -> bool {
    local.unwrap_or(false) || !remote.unwrap_or(false)
}

pub(crate) fn include_remote_source(local: Option<bool>, remote: Option<bool>) -> bool {
    remote.unwrap_or(false) || !local.unwrap_or(false)
}

pub(crate) fn matches_tag_timeline_filters(
    tags: &[String],
    primary_tag: &str,
    query: &TagTimelineQuery,
) -> bool {
    let tag_set = tags.iter().map(|tag| tag.as_str()).collect::<HashSet<_>>();
    if !tag_set.contains(primary_tag) {
        return false;
    }

    if let Some(any_tags) = query.any.as_ref() {
        let normalized = any_tags
            .iter()
            .map(|tag| normalize_hashtag(tag))
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        if !normalized.is_empty() && !normalized.iter().any(|tag| tag_set.contains(tag.as_str())) {
            return false;
        }
    }

    if let Some(all_tags) = query.all.as_ref()
        && !all_tags
            .iter()
            .map(|tag| normalize_hashtag(tag))
            .filter(|tag| !tag.is_empty())
            .all(|tag| tag_set.contains(tag.as_str()))
    {
        return false;
    }

    if let Some(none_tags) = query.none.as_ref()
        && none_tags
            .iter()
            .map(|tag| normalize_hashtag(tag))
            .filter(|tag| !tag.is_empty())
            .any(|tag| tag_set.contains(tag.as_str()))
    {
        return false;
    }

    true
}
