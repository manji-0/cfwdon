use super::normalize_hashtag;
use serde::Deserialize;
use std::collections::HashSet;
use url::Url;
use worker::{Request, Result};

use crate::D1Database;
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub(crate) struct TimelinePaginationQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
}

impl TimelinePaginationQuery {
    fn from_parts(
        limit: Option<u32>,
        max_id: Option<String>,
        since_id: Option<String>,
        min_id: Option<String>,
    ) -> Self {
        Self {
            limit,
            max_id,
            since_id,
            min_id,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicTimelineQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
    pub(crate) local: Option<bool>,
    pub(crate) remote: Option<bool>,
    pub(crate) only_media: Option<bool>,
}

impl PublicTimelineQuery {
    pub(crate) fn pagination(&self) -> TimelinePaginationQuery {
        TimelinePaginationQuery::from_parts(
            self.limit,
            self.max_id.clone(),
            self.since_id.clone(),
            self.min_id.clone(),
        )
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TagTimelineQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
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

impl TagTimelineQuery {
    pub(crate) fn pagination(&self) -> TimelinePaginationQuery {
        TimelinePaginationQuery::from_parts(
            self.limit,
            self.max_id.clone(),
            self.since_id.clone(),
            self.min_id.clone(),
        )
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct HomeTimelineQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
}

impl HomeTimelineQuery {
    pub(crate) fn pagination(&self) -> TimelinePaginationQuery {
        TimelinePaginationQuery::from_parts(
            self.limit,
            self.max_id.clone(),
            self.since_id.clone(),
            self.min_id.clone(),
        )
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct LinkTimelineQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
    pub(crate) url: Option<String>,
}

impl LinkTimelineQuery {
    pub(crate) fn pagination(&self) -> TimelinePaginationQuery {
        TimelinePaginationQuery::from_parts(
            self.limit,
            self.max_id.clone(),
            self.since_id.clone(),
            self.min_id.clone(),
        )
    }
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

pub(crate) fn derive_link_timeline_match_urls(value: &str) -> Vec<String> {
    let target = value.trim();
    if target.is_empty() {
        return Vec::new();
    }

    let mut urls = vec![target.to_owned()];
    let Ok(parsed) = Url::parse(target) else {
        return urls;
    };

    let mut normalized = parsed;
    normalized.set_fragment(None);
    push_unique_link_timeline_url(&mut urls, normalized.to_string());
    if let Some(variant) = remove_tracking_query_params_url(&normalized) {
        push_unique_link_timeline_url(&mut urls, variant.clone());
        if let Ok(parsed_variant) = Url::parse(&variant)
            && let Some(toggled) = toggle_trailing_slash_url(&parsed_variant)
        {
            push_unique_link_timeline_url(&mut urls, toggled);
        }
    }
    if let Some(variant) = toggle_trailing_slash_url(&normalized) {
        push_unique_link_timeline_url(&mut urls, variant);
    }

    urls
}

pub(crate) fn canonicalize_link_timeline_url(value: &str) -> Option<String> {
    let target = value.trim();
    if target.is_empty() {
        return None;
    }

    let Ok(mut parsed) = Url::parse(target) else {
        return Some(target.to_owned());
    };

    parsed.set_fragment(None);
    if let Some(variant) = remove_tracking_query_params_url(&parsed) {
        return Some(variant);
    }

    Some(parsed.to_string())
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

    let (max_timestamp, min_timestamp) = futures_util::try_join!(
        resolve_timeline_cursor_timestamp(db, max_id.as_deref()),
        resolve_timeline_cursor_timestamp(db, min_id.as_deref()),
    )?;

    Ok(ResolvedTimelineCursor {
        max_timestamp,
        min_timestamp,
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

fn push_unique_link_timeline_url(urls: &mut Vec<String>, value: String) {
    if !value.is_empty() && !urls.iter().any(|url| url == &value) {
        urls.push(value);
    }
}

fn toggle_trailing_slash_url(url: &Url) -> Option<String> {
    let path = url.path();
    if path == "/" || path.is_empty() {
        return None;
    }

    let toggled_path = if path.ends_with('/') {
        path.trim_end_matches('/').to_owned()
    } else {
        format!("{path}/")
    };
    if toggled_path.is_empty() {
        return None;
    }

    let mut variant = url.clone();
    variant.set_path(&toggled_path);
    Some(variant.to_string())
}

fn remove_tracking_query_params_url(url: &Url) -> Option<String> {
    let mut filtered = Vec::new();
    let mut removed = false;

    for (key, value) in url.query_pairs() {
        let key_lower = key.to_ascii_lowercase();
        if key_lower.starts_with("utm_")
            || matches!(
                key_lower.as_str(),
                "fbclid" | "gclid" | "mc_cid" | "mc_eid" | "igshid"
            )
        {
            removed = true;
            continue;
        }
        filtered.push((key.into_owned(), value.into_owned()));
    }

    if !removed {
        return None;
    }

    let mut variant = url.clone();
    {
        let mut query = variant.query_pairs_mut();
        query.clear();
        for (key, value) in filtered {
            query.append_pair(&key, &value);
        }
    }
    if variant.query() == Some("") {
        variant.set_query(None);
    }

    Some(variant.to_string())
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
