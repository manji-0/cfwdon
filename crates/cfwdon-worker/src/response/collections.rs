use crate::{MastodonAccountResponse, MastodonStatusResponse};
use serde::Serialize;

pub(crate) const UNAUTH_CONTEXT_ANCESTOR_LIMIT: usize = 40;
pub(crate) const UNAUTH_CONTEXT_DESCENDANT_LIMIT: usize = 60;
pub(crate) const UNAUTH_CONTEXT_DESCENDANT_MAX_DEPTH: usize = 20;
pub(crate) const AUTH_CONTEXT_LIMIT: usize = 4096;

#[derive(Debug, Default, Serialize)]
pub(crate) struct MastodonSearchResponse {
    pub(crate) accounts: Vec<MastodonAccountResponse>,
    pub(crate) statuses: Vec<MastodonStatusResponse>,
    pub(crate) hashtags: Vec<MastodonTagResponse>,
    pub(crate) collections: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct MastodonContextResponse {
    pub(crate) ancestors: Vec<MastodonStatusResponse>,
    pub(crate) descendants: Vec<MastodonStatusResponse>,
}

pub(crate) fn context_descendant_max_depth(is_authenticated: bool) -> Option<usize> {
    if is_authenticated {
        None
    } else {
        Some(UNAUTH_CONTEXT_DESCENDANT_MAX_DEPTH)
    }
}

pub(crate) fn trim_context_ancestors<T>(mut ancestors: Vec<T>, is_authenticated: bool) -> Vec<T> {
    let limit = if is_authenticated {
        AUTH_CONTEXT_LIMIT
    } else {
        UNAUTH_CONTEXT_ANCESTOR_LIMIT
    };

    if ancestors.len() > limit {
        ancestors = ancestors.into_iter().rev().take(limit).collect::<Vec<_>>();
        ancestors.reverse();
    }
    ancestors
}

pub(crate) fn trim_context_descendants<T>(descendants: Vec<T>, is_authenticated: bool) -> Vec<T> {
    descendants
        .into_iter()
        .take(if is_authenticated {
            AUTH_CONTEXT_LIMIT
        } else {
            UNAUTH_CONTEXT_DESCENDANT_LIMIT
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonTagResponse {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) history: Vec<MastodonTagHistoryEntry>,
    pub(crate) following: bool,
    pub(crate) featured: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonTagHistoryEntry {
    pub(crate) day: String,
    pub(crate) uses: String,
    pub(crate) accounts: String,
}
