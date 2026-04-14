use super::normalize_hashtag;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TagTimelineQuery {
    pub(crate) limit: Option<u32>,
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
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) _max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) _since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) _min_id: Option<String>,
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
