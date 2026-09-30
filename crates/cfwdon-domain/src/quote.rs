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

    pub fn quote_state_for_local_publish(has_quote: bool, target_exists_locally: bool) -> Self {
        Self::initial_for_quote_target(has_quote, target_exists_locally)
    }

    pub fn quote_state_for_remote_publish(
        has_quote: bool,
        target_exists_locally: bool,
        blocked_by_owner: bool,
        policy_allows: bool,
    ) -> Self {
        if !has_quote || !target_exists_locally {
            Self::Accepted
        } else {
            Self::remote_for_target(blocked_by_owner, policy_allows)
        }
    }

    pub fn quote_state_after_remote_upsert(current: Self, incoming: Self) -> Self {
        match current {
            Self::Rejected | Self::Revoked => current,
            _ => incoming,
        }
    }

    pub fn quote_state_after_owner_approve(current: Self) -> Self {
        match current {
            Self::Pending => Self::Accepted,
            other => other,
        }
    }

    pub fn quote_state_after_owner_reject(current: Self) -> Self {
        match current {
            Self::Pending => Self::Rejected,
            other => other,
        }
    }

    pub fn quote_state_after_revoke(_current: Self) -> Self {
        Self::Revoked
    }

    pub fn counts_in_accepted_quotes_timeline(self) -> bool {
        self == Self::Accepted
    }

    pub fn shows_quote_placeholder(self) -> bool {
        self == Self::Pending
    }

    pub fn quote_state_after_owner_action(self, action: OwnerQuoteAction) -> Self {
        match action {
            OwnerQuoteAction::Approve => Self::quote_state_after_owner_approve(self),
            OwnerQuoteAction::Reject => Self::quote_state_after_owner_reject(self),
            OwnerQuoteAction::Revoke => Self::quote_state_after_revoke(self),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OwnerQuoteAction {
    Approve,
    Reject,
    Revoke,
}

pub fn merged_quote_state_for_remote_upsert(
    current: QuoteState,
    incoming: QuoteState,
) -> QuoteState {
    QuoteState::quote_state_after_remote_upsert(current, incoming)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_action_transitions_match_api_paths() {
        assert_eq!(
            QuoteState::Pending.quote_state_after_owner_action(OwnerQuoteAction::Approve),
            QuoteState::Accepted
        );
        assert_eq!(
            QuoteState::Pending.quote_state_after_owner_action(OwnerQuoteAction::Reject),
            QuoteState::Rejected
        );
        assert_eq!(
            QuoteState::Accepted.quote_state_after_owner_action(OwnerQuoteAction::Revoke),
            QuoteState::Revoked
        );
    }

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

    #[test]
    fn remote_upsert_preserves_owner_terminal_states() {
        assert_eq!(
            QuoteState::quote_state_after_remote_upsert(QuoteState::Revoked, QuoteState::Accepted,),
            QuoteState::Revoked
        );
        assert_eq!(
            QuoteState::quote_state_after_remote_upsert(QuoteState::Rejected, QuoteState::Accepted,),
            QuoteState::Rejected
        );
        assert_eq!(
            QuoteState::quote_state_after_remote_upsert(QuoteState::Pending, QuoteState::Accepted,),
            QuoteState::Accepted
        );
    }

    #[test]
    fn owner_approval_transitions_only_from_pending() {
        assert_eq!(
            QuoteState::quote_state_after_owner_approve(QuoteState::Pending),
            QuoteState::Accepted
        );
        assert_eq!(
            QuoteState::quote_state_after_owner_approve(QuoteState::Rejected),
            QuoteState::Rejected
        );
    }

    #[test]
    fn remote_quote_state_for_local_target_matches_policy_rules() {
        use crate::QuoteState;

        fn remote_quote_state_for_local_target(
            status: &crate::LocalStatus,
            remote_actor_follows_owner: bool,
            blocked_by_owner: bool,
        ) -> &'static str {
            let policy = status.effective_quote_approval_policy();
            QuoteState::remote_for_target(
                blocked_by_owner,
                policy.allows_quote(false, remote_actor_follows_owner),
            )
            .as_str()
        }

        let mut status = crate::LocalStatus {
            id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: None,
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>hello</p>".to_owned(),
            text: "hello".to_owned(),
            spoiler_text: String::new(),
            visibility: crate::Visibility::Public,
            sensitive: false,
            language: Some("en".to_owned()),
            quote_approval_policy: Some(crate::QuoteApprovalPolicy::Public),
            quote_state: crate::QuoteState::Accepted,
            application_id: None,
            card_json: None,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            updated_at: None,
        };

        assert_eq!(
            remote_quote_state_for_local_target(&status, false, false),
            "accepted"
        );

        status.quote_approval_policy = Some(crate::QuoteApprovalPolicy::Followers);
        assert_eq!(
            remote_quote_state_for_local_target(&status, true, false),
            "accepted"
        );
        assert_eq!(
            remote_quote_state_for_local_target(&status, false, false),
            "pending"
        );

        status.quote_approval_policy = Some(crate::QuoteApprovalPolicy::Nobody);
        assert_eq!(
            remote_quote_state_for_local_target(&status, true, false),
            "pending"
        );

        status.visibility = crate::Visibility::FollowersOnly;
        status.quote_approval_policy = Some(crate::QuoteApprovalPolicy::Public);
        assert_eq!(
            remote_quote_state_for_local_target(&status, true, false),
            "pending"
        );
        assert_eq!(
            remote_quote_state_for_local_target(&status, true, true),
            "rejected"
        );
    }
}
