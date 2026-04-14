use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct MastodonAccountResponse {
    pub(crate) id: String,
    pub(crate) username: String,
    pub(crate) acct: String,
    pub(crate) display_name: String,
    pub(crate) locked: bool,
    pub(crate) bot: bool,
    pub(crate) created_at: String,
    pub(crate) note: String,
    pub(crate) url: String,
    pub(crate) avatar: String,
    pub(crate) avatar_static: String,
    pub(crate) header: String,
    pub(crate) header_static: String,
    pub(crate) fields: Vec<serde_json::Value>,
    pub(crate) followers_count: u64,
    pub(crate) following_count: u64,
    pub(crate) statuses_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<MastodonAccountSource>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonAccountSource {
    pub(crate) note: String,
    pub(crate) fields: Vec<serde_json::Value>,
    pub(crate) privacy: String,
    pub(crate) sensitive: bool,
    pub(crate) language: String,
    pub(crate) follow_requests_count: u64,
    pub(crate) hide_collections: Option<bool>,
    pub(crate) discoverable: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonStatusResponse {
    pub(crate) id: String,
    pub(crate) created_at: String,
    pub(crate) in_reply_to_id: Option<String>,
    pub(crate) in_reply_to_account_id: Option<String>,
    pub(crate) sensitive: bool,
    pub(crate) spoiler_text: String,
    pub(crate) visibility: String,
    pub(crate) language: Option<String>,
    pub(crate) uri: String,
    pub(crate) url: String,
    pub(crate) replies_count: u64,
    pub(crate) reblogs_count: u64,
    pub(crate) favourites_count: u64,
    pub(crate) favourited: bool,
    pub(crate) reblogged: bool,
    pub(crate) muted: bool,
    pub(crate) bookmarked: bool,
    pub(crate) pinned: bool,
    pub(crate) content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
    pub(crate) reblog: Option<serde_json::Value>,
    pub(crate) application: Option<serde_json::Value>,
    pub(crate) account: MastodonAccountResponse,
    pub(crate) media_attachments: Vec<serde_json::Value>,
    pub(crate) mentions: Vec<serde_json::Value>,
    pub(crate) tags: Vec<serde_json::Value>,
    pub(crate) emojis: Vec<serde_json::Value>,
    pub(crate) card: Option<serde_json::Value>,
    pub(crate) poll: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonReportResponse {
    pub(crate) id: String,
    pub(crate) action_taken: bool,
    pub(crate) action_taken_at: Option<String>,
    pub(crate) category: String,
    pub(crate) comment: String,
    pub(crate) forwarded: bool,
    pub(crate) created_at: String,
    pub(crate) status_ids: Option<Vec<String>>,
    pub(crate) target_account: MastodonAccountResponse,
    pub(crate) rule_ids: Option<Vec<String>>,
}

pub(crate) use crate::response_collections::*;
