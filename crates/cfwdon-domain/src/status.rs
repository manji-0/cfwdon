use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Unlisted,
    FollowersOnly,
    Direct,
}

impl Visibility {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "public" => Some(Self::Public),
            "unlisted" => Some(Self::Unlisted),
            "private" => Some(Self::FollowersOnly),
            "direct" => Some(Self::Direct),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Unlisted => "unlisted",
            Self::FollowersOnly => "private",
            Self::Direct => "direct",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PollDraft {
    pub options: Vec<String>,
    pub expires_in_seconds: u64,
    pub multiple: bool,
    pub hide_totals: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusDraft {
    pub text: String,
    pub visibility: Visibility,
    pub spoiler_text: String,
    pub sensitive: bool,
    pub language: Option<String>,
    pub quote_approval_policy: Option<String>,
    pub in_reply_to_id: Option<String>,
    pub media_ids: Vec<String>,
    pub poll: Option<PollDraft>,
}
