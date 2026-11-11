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

/// The instance's `registrations_mode`, named as Mastodon names it.
///
/// The default is `Approved`: an instance whose operator never chose a mode
/// holds sign-ups for review instead of handing out working accounts.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum RegistrationsMode {
    Open,
    /// Every sign-up waits for an admin.
    #[default]
    Approved,
    /// Mastodon's `none`: no sign-ups at all.
    #[serde(rename = "none")]
    Closed,
}

impl RegistrationsMode {
    pub const ALL: [Self; 3] = [Self::Open, Self::Approved, Self::Closed];

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "approved" => Some(Self::Approved),
            "none" => Some(Self::Closed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Approved => "approved",
            Self::Closed => "none",
        }
    }

    /// `registrations` (v1) and `registrations.enabled` (v2) in the instance
    /// documents.
    pub fn registrations_enabled(self) -> bool {
        self != Self::Closed
    }

    /// `approval_required` in the instance documents.
    pub fn approval_required(self) -> bool {
        self == Self::Approved
    }
}

/// What the `REGISTRATIONS_MODE` setting held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegistrationsModeSetting {
    /// Unset or blank.
    Unset,
    Named(RegistrationsMode),
    /// A value that names no mode, such as a typo of `none`.
    Unrecognized,
}

impl RegistrationsModeSetting {
    pub const ALL: [Self; 5] = [
        Self::Unset,
        Self::Named(RegistrationsMode::Open),
        Self::Named(RegistrationsMode::Approved),
        Self::Named(RegistrationsMode::Closed),
        Self::Unrecognized,
    ];

    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => Self::Unset,
            Some(value) => {
                RegistrationsMode::from_name(value).map_or(Self::Unrecognized, Self::Named)
            }
        }
    }

    /// Only an explicit `open` takes sign-ups without review. Unset and
    /// unrecognized settings fail closed to `approved`, which still takes
    /// sign-ups but holds each one for an admin.
    pub fn mode(self) -> RegistrationsMode {
        match self {
            Self::Named(mode) => mode,
            Self::Unset | Self::Unrecognized => RegistrationsMode::Approved,
        }
    }
}

/// Mastodon's `UserInviteRequest::TEXT_SIZE_LIMIT`: the longest sign-up
/// `reason` it takes, in characters.
pub const SIGN_UP_REASON_MAX_CHARS: usize = 420;

/// Whether a sign-up `reason` is over [`SIGN_UP_REASON_MAX_CHARS`]; Mastodon
/// answers such a sign-up with `422`.
pub fn sign_up_reason_too_long(reason: &str) -> bool {
    reason.chars().count() > SIGN_UP_REASON_MAX_CHARS
}

/// A sign-up the instance refuses outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignUpRefused;

/// Mastodon's `RegistrationHelper#allowed_registration?` refuses a sign-up
/// when registrations are closed or the address is in a `sign_up_block`
/// range; `User#set_approved` then holds the account when the address needs
/// approval or the instance approves every sign-up.
pub fn sign_up_approval(
    mode: RegistrationsMode,
    ip: IpSignUpDecision,
) -> Result<AccountApproval, SignUpRefused> {
    match (ip, mode) {
        (IpSignUpDecision::Blocked, _) | (_, RegistrationsMode::Closed) => Err(SignUpRefused),
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
            (RegistrationsMode::Closed, Open, Err(SignUpRefused)),
            (
                RegistrationsMode::Closed,
                RequiresApproval,
                Err(SignUpRefused),
            ),
            (RegistrationsMode::Closed, Blocked, Err(SignUpRefused)),
        ] {
            assert_eq!(sign_up_approval(mode, ip), expected, "{mode:?} {ip:?}");
        }
    }

    #[test]
    fn only_an_explicit_open_setting_opens_registrations() {
        for (value, expected) in [
            (None, RegistrationsModeSetting::Unset),
            (Some(""), RegistrationsModeSetting::Unset),
            (Some("  "), RegistrationsModeSetting::Unset),
            (
                Some("open"),
                RegistrationsModeSetting::Named(RegistrationsMode::Open),
            ),
            (
                Some(" Approved "),
                RegistrationsModeSetting::Named(RegistrationsMode::Approved),
            ),
            (
                Some("none"),
                RegistrationsModeSetting::Named(RegistrationsMode::Closed),
            ),
            (Some("closed"), RegistrationsModeSetting::Unrecognized),
            (Some("aproved"), RegistrationsModeSetting::Unrecognized),
        ] {
            assert_eq!(
                RegistrationsModeSetting::parse(value),
                expected,
                "{value:?}"
            );
        }
        for setting in RegistrationsModeSetting::ALL {
            assert_eq!(
                setting.mode() == RegistrationsMode::Open,
                setting == RegistrationsModeSetting::Named(RegistrationsMode::Open),
                "{setting:?}"
            );
        }
        assert_eq!(RegistrationsMode::default(), RegistrationsMode::Approved);
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
    fn sign_up_reason_is_capped_in_characters() {
        assert!(!sign_up_reason_too_long(
            &"あ".repeat(SIGN_UP_REASON_MAX_CHARS)
        ));
        assert!(sign_up_reason_too_long(
            &"a".repeat(SIGN_UP_REASON_MAX_CHARS + 1)
        ));
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
        assert_eq!(
            RegistrationsMode::from_name("none"),
            Some(RegistrationsMode::Closed)
        );
        assert_eq!(RegistrationsMode::from_name("closed"), None);
    }
}
