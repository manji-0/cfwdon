use crate::error::{QuoteApprovalPolicyError, QuoteStateError};
use crate::status::Visibility;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuoteApprovalPolicy {
    Public,
    Followers,
    Nobody,
}

impl QuoteApprovalPolicy {
    pub fn parse(value: &str) -> Result<Self, QuoteApprovalPolicyError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "public" => Ok(Self::Public),
            "followers" => Ok(Self::Followers),
            "nobody" => Ok(Self::Nobody),
            _ => Err(QuoteApprovalPolicyError::Unknown),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Followers => "followers",
            Self::Nobody => "nobody",
        }
    }

    pub fn allows_quote(self, is_owner: bool, is_follower: bool) -> bool {
        if is_owner {
            return true;
        }
        match self {
            Self::Public => true,
            Self::Followers => is_follower,
            Self::Nobody => false,
        }
    }

    pub fn for_status_visibility(
        visibility: Visibility,
        requested: Option<Self>,
        account_default: Self,
    ) -> Self {
        if matches!(visibility, Visibility::FollowersOnly | Visibility::Direct) {
            Self::Nobody
        } else {
            requested.unwrap_or(account_default)
        }
    }

    pub fn for_stored_visibility(visibility: &str, stored_policy: Option<&str>) -> Self {
        if matches!(visibility, "private" | "direct") {
            Self::Nobody
        } else {
            stored_policy
                .map(Self::parse)
                .transpose()
                .ok()
                .flatten()
                .unwrap_or(Self::Public)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuoteState {
    Accepted,
    Pending,
    Rejected,
    Revoked,
}

impl QuoteState {
    pub fn parse(value: &str) -> Result<Self, QuoteStateError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "accepted" => Ok(Self::Accepted),
            "pending" => Ok(Self::Pending),
            "rejected" => Ok(Self::Rejected),
            "revoked" => Ok(Self::Revoked),
            _ => Err(QuoteStateError::Unknown),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Pending => "pending",
            Self::Rejected => "rejected",
            Self::Revoked => "revoked",
        }
    }

    pub fn effective_for_stored(quote_of_uri: Option<&str>, stored: QuoteState) -> QuoteState {
        if quote_of_uri.is_none() {
            QuoteState::Accepted
        } else {
            stored
        }
    }

    pub fn is_visible(self) -> bool {
        !matches!(self, Self::Revoked)
    }

    pub fn initial_for_quote_target(has_quote: bool, target_exists_locally: bool) -> Self {
        if !has_quote || target_exists_locally {
            Self::Accepted
        } else {
            Self::Pending
        }
    }

    pub fn remote_for_target(blocked_by_owner: bool, policy_allows: bool) -> Self {
        if blocked_by_owner {
            Self::Rejected
        } else if policy_allows {
            Self::Accepted
        } else {
            Self::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_policy_for_private_visibility_is_nobody() {
        assert_eq!(
            QuoteApprovalPolicy::for_status_visibility(
                Visibility::FollowersOnly,
                Some(QuoteApprovalPolicy::Public),
                QuoteApprovalPolicy::Followers,
            ),
            QuoteApprovalPolicy::Nobody
        );
    }

    #[test]
    fn quote_state_remote_for_blocked_target_is_rejected() {
        assert_eq!(
            QuoteState::remote_for_target(true, true),
            QuoteState::Rejected
        );
    }
}
