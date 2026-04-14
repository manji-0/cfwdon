use super::{MastodonAccountResponse, MastodonStatusResponse};
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub(crate) struct MastodonSearchResponse {
    pub(crate) accounts: Vec<MastodonAccountResponse>,
    pub(crate) statuses: Vec<MastodonStatusResponse>,
    pub(crate) hashtags: Vec<MastodonTagResponse>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct MastodonContextResponse {
    pub(crate) ancestors: Vec<MastodonStatusResponse>,
    pub(crate) descendants: Vec<MastodonStatusResponse>,
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
