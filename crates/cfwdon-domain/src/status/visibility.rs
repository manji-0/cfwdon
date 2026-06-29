use crate::error::VisibilityError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Unlisted,
    FollowersOnly,
    Direct,
}

impl Visibility {
    pub fn parse(value: &str) -> Result<Self, VisibilityError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "public" => Ok(Self::Public),
            "unlisted" => Ok(Self::Unlisted),
            "private" => Ok(Self::FollowersOnly),
            "direct" => Ok(Self::Direct),
            _ => Err(VisibilityError::Unknown),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Unlisted => "unlisted",
            Self::FollowersOnly => "private",
            Self::Direct => "direct",
        }
    }

    pub fn is_restricted(self) -> bool {
        matches!(self, Self::FollowersOnly | Self::Direct)
    }
}
