//! Mastodon's registration approval: `Setting.registrations_mode`,
//! `User#set_approved`, `Api::BaseController#require_user!`, and the
//! `UserPolicy#approve?` / `#reject?` review of a pending account.

use crate::ip_block::IpSignUpDecision;

/// Whether an account's owner may use it yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccountApproval {
    Approved,
    /// Held for an admin to approve or reject.
    Pending,
}

impl AccountApproval {
    /// Mastodon answers every API request from a pending account with 403
    /// "Your login is currently pending approval", whatever token it holds.
    pub fn grants_api_access(self) -> bool {
        self == Self::Approved
    }

    /// An admin's review. Mastodon only reviews a pending account; an
    /// approved one has nothing to approve or reject (`None`, a 403).
    pub fn review(self, review: ApprovalReview) -> Option<ReviewOutcome> {
        match (self, review) {
            (Self::Approved, _) => None,
            (Self::Pending, ApprovalReview::Approve) => Some(ReviewOutcome::Approved),
            (Self::Pending, ApprovalReview::Reject) => Some(ReviewOutcome::Rejected),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApprovalReview {
    Approve,
    Reject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReviewOutcome {
    /// The account becomes [`AccountApproval::Approved`].
    Approved,
    /// The account is deleted, freeing its username and e-mail.
    Rejected,
}

/// The instance's `registrations_mode`, short of Mastodon's `none`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RegistrationsMode {
    #[default]
    Open,
    /// Every sign-up waits for an admin.
    Approved,
}

impl RegistrationsMode {
    pub const ALL: [Self; 2] = [Self::Open, Self::Approved];

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "approved" => Some(Self::Approved),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Approved => "approved",
        }
    }

    /// `approval_required` in the instance document.
    pub fn approval_required(self) -> bool {
        self == Self::Approved
    }
}

/// A sign-up the instance refuses outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignUpRefused;

/// Mastodon's `RegistrationHelper#allowed_registration?` refuses a
/// `sign_up_block` range; `User#set_approved` then holds the account when the
/// address needs approval or the instance approves every sign-up.
pub fn sign_up_approval(
    mode: RegistrationsMode,
    ip: IpSignUpDecision,
) -> Result<AccountApproval, SignUpRefused> {
    match (ip, mode) {
        (IpSignUpDecision::Blocked, _) => Err(SignUpRefused),
        (IpSignUpDecision::RequiresApproval, _) | (_, RegistrationsMode::Approved) => {
            Ok(AccountApproval::Pending)
        }
        (IpSignUpDecision::Open, RegistrationsMode::Open) => Ok(AccountApproval::Approved),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_up_approval_matches_mastodon() {
        use AccountApproval::{Approved, Pending};
        use IpSignUpDecision::{Blocked, Open, RequiresApproval};
        for (mode, ip, expected) in [
            (RegistrationsMode::Open, Open, Ok(Approved)),
            (RegistrationsMode::Open, RequiresApproval, Ok(Pending)),
            (RegistrationsMode::Open, Blocked, Err(SignUpRefused)),
            (RegistrationsMode::Approved, Open, Ok(Pending)),
            (RegistrationsMode::Approved, RequiresApproval, Ok(Pending)),
            (RegistrationsMode::Approved, Blocked, Err(SignUpRefused)),
        ] {
            assert_eq!(sign_up_approval(mode, ip), expected, "{mode:?} {ip:?}");
        }
    }

    #[test]
    fn only_a_pending_account_is_reviewed() {
        for review in [ApprovalReview::Approve, ApprovalReview::Reject] {
            assert_eq!(AccountApproval::Approved.review(review), None);
        }
        assert_eq!(
            AccountApproval::Pending.review(ApprovalReview::Approve),
            Some(ReviewOutcome::Approved)
        );
        assert_eq!(
            AccountApproval::Pending.review(ApprovalReview::Reject),
            Some(ReviewOutcome::Rejected)
        );
        assert!(AccountApproval::Approved.grants_api_access());
        assert!(!AccountApproval::Pending.grants_api_access());
    }

    #[test]
    fn registrations_mode_names_round_trip() {
        for mode in RegistrationsMode::ALL {
            assert_eq!(RegistrationsMode::from_name(mode.as_str()), Some(mode));
        }
        assert_eq!(
            RegistrationsMode::from_name(" Approved "),
            Some(RegistrationsMode::Approved)
        );
        assert_eq!(RegistrationsMode::from_name("none"), None);
    }
}
