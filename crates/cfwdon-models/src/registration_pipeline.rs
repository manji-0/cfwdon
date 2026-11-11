use cfwdon_domain::{
    AccountApproval, AccountId, AccountKeyMaterial, ApprovalReview, ComposingRegistration,
    IpSignUpDecision, LocalAccount, QuoteApprovalPolicy, RegistrationEvent, RegistrationFieldIssue,
    RegistrationUniquenessFacts, RegistrationValidationErrors, RegistrationsMode, ReviewOutcome,
    Visibility, registration_uniqueness_errors, sign_up_approval,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RegistrationPipelineModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RegistrationStage {
    Composing,
    /// The client address is in a `sign_up_block` range: refused before the
    /// form is read.
    Refused,
    Validated,
    ValidationFailed,
    Registered,
    Provisioned,
    /// An admin rejected the pending account, which deleted it.
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TextFieldInput {
    Missing,
    Blank,
    Invalid,
    Valid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RegistrationPipelineModelState {
    pub(crate) stage: RegistrationStage,
    pub(crate) username: TextFieldInput,
    pub(crate) email: TextFieldInput,
    pub(crate) password_present: bool,
    pub(crate) agreement: bool,
    pub(crate) username_taken: bool,
    pub(crate) email_taken: bool,
    pub(crate) mode: RegistrationsMode,
    pub(crate) ip: IpSignUpDecision,
    /// Set when the account is inserted; `None` before and after rejection.
    pub(crate) approval: Option<AccountApproval>,
    /// An admin approved the account.
    pub(crate) reviewed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RegistrationPipelineAction {
    CycleUsername,
    CycleEmail,
    TogglePassword,
    ToggleAgreement,
    ToggleUsernameTaken,
    ToggleEmailTaken,
    Validate,
    Register,
    Provision,
    Approve,
    Reject,
}

impl RegistrationPipelineModel {
    fn field_value(input: TextFieldInput, kind: FieldKind) -> Option<String> {
        registration_pipeline_field_value(input, kind)
    }

    fn composing_registration(state: &RegistrationPipelineModelState) -> ComposingRegistration {
        registration_pipeline_composing(state)
    }

    fn expected_validation_errors(
        state: &RegistrationPipelineModelState,
    ) -> RegistrationValidationErrors {
        registration_pipeline_validation_errors(state)
    }

    fn fixture_keys() -> AccountKeyMaterial {
        registration_pipeline_fixture_keys()
    }

    fn fixture_account_id() -> AccountId {
        registration_pipeline_fixture_account_id()
    }

    fn cycle_field(input: TextFieldInput) -> TextFieldInput {
        match input {
            TextFieldInput::Missing => TextFieldInput::Blank,
            TextFieldInput::Blank => TextFieldInput::Invalid,
            TextFieldInput::Invalid => TextFieldInput::Valid,
            TextFieldInput::Valid => TextFieldInput::Missing,
        }
    }

    fn provisioned_account(state: &RegistrationPipelineModelState) -> LocalAccount {
        registration_pipeline_provisioned_account(state)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FieldKind {
    Username,
    Email,
}

fn registration_pipeline_field_value(input: TextFieldInput, kind: FieldKind) -> Option<String> {
    match (input, kind) {
        (TextFieldInput::Missing, _) => None,
        (TextFieldInput::Blank, _) => Some(String::new()),
        (TextFieldInput::Invalid, FieldKind::Username) => Some("bad name!".to_owned()),
        (TextFieldInput::Invalid, FieldKind::Email) => Some("not-an-email".to_owned()),
        (TextFieldInput::Valid, FieldKind::Username) => Some("alice".to_owned()),
        (TextFieldInput::Valid, FieldKind::Email) => Some("alice@example.com".to_owned()),
    }
}

pub(crate) fn registration_pipeline_composing(
    state: &RegistrationPipelineModelState,
) -> ComposingRegistration {
    ComposingRegistration {
        username: registration_pipeline_field_value(state.username, FieldKind::Username),
        email: registration_pipeline_field_value(state.email, FieldKind::Email),
        password_present: state.password_present,
        agreement: state.agreement.then_some(true),
    }
}

pub(crate) fn registration_pipeline_validation_errors(
    state: &RegistrationPipelineModelState,
) -> RegistrationValidationErrors {
    let mut errors = RegistrationValidationErrors::default();

    match state.username {
        TextFieldInput::Missing | TextFieldInput::Blank => {
            errors.username = Some(RegistrationFieldIssue::Blank);
        }
        TextFieldInput::Invalid => {
            errors.username = Some(RegistrationFieldIssue::InvalidFormat);
        }
        TextFieldInput::Valid => {}
    }

    match state.email {
        TextFieldInput::Missing | TextFieldInput::Blank => {
            errors.email = Some(RegistrationFieldIssue::Blank);
        }
        TextFieldInput::Invalid => {
            errors.email = Some(RegistrationFieldIssue::InvalidFormat);
        }
        TextFieldInput::Valid => {}
    }

    if !state.password_present {
        errors.password = Some(RegistrationFieldIssue::Blank);
    }
    if !state.agreement {
        errors.agreement = Some(RegistrationFieldIssue::MustBeAccepted);
    }

    errors
}

pub(crate) fn registration_pipeline_fixture_keys() -> AccountKeyMaterial {
    AccountKeyMaterial {
        private_key_jwk: r#"{"kty":"RSA"}"#.to_owned(),
        public_key_pem: "pem-fixture".to_owned(),
    }
}

pub(crate) fn registration_pipeline_fixture_account_id() -> AccountId {
    AccountId::new("acct-model").expect("fixture account id")
}

pub(crate) fn registration_pipeline_uniqueness_facts(
    state: &RegistrationPipelineModelState,
) -> RegistrationUniquenessFacts {
    RegistrationUniquenessFacts {
        username_taken: state.username_taken,
        email_taken: state.email_taken,
    }
}

pub(crate) fn registration_pipeline_uniqueness_errors(
    state: &RegistrationPipelineModelState,
) -> RegistrationValidationErrors {
    registration_uniqueness_errors(registration_pipeline_uniqueness_facts(state))
}

/// Mirrors register guard: validated inputs and no username/email collision.
pub(crate) fn registration_pipeline_register_allowed(
    state: &RegistrationPipelineModelState,
) -> bool {
    state.stage == RegistrationStage::Validated
        && registration_pipeline_sign_up_approval(state).is_some()
        && registration_pipeline_validation_errors(state).is_empty()
        && registration_pipeline_uniqueness_errors(state).is_empty()
}

/// Mirrors `finalize_registration_validation` success gate.
pub(crate) fn registration_pipeline_finalize_allowed(
    state: &RegistrationPipelineModelState,
) -> bool {
    registration_pipeline_validation_errors(state).is_empty()
        && registration_pipeline_uniqueness_errors(state).is_empty()
}

/// Mirrors the sign-up handler: the IP decision refuses a blocked address
/// before the form is read, then `account_registration_api_details` validates.
pub(crate) fn apply_registration_pipeline_validate(
    state: &RegistrationPipelineModelState,
) -> RegistrationStage {
    if registration_pipeline_sign_up_approval(state).is_none() {
        RegistrationStage::Refused
    } else if registration_pipeline_validation_errors(state).is_empty() {
        RegistrationStage::Validated
    } else {
        RegistrationStage::ValidationFailed
    }
}

/// `sign_up_approval` for the state's mode and address; `None` when refused.
pub(crate) fn registration_pipeline_sign_up_approval(
    state: &RegistrationPipelineModelState,
) -> Option<AccountApproval> {
    sign_up_approval(state.mode, state.ip).ok()
}

/// Whether a credential for the account authorizes API requests: the token
/// row and the Auth0 sign-in both decode a pending account as no account.
pub(crate) fn registration_pipeline_api_access(state: &RegistrationPipelineModelState) -> bool {
    state.stage == RegistrationStage::Provisioned
        && state
            .approval
            .is_some_and(AccountApproval::grants_api_access)
}

/// The admin review the account allows, as the approve/reject handlers ask it.
pub(crate) fn registration_pipeline_review(
    state: &RegistrationPipelineModelState,
    review: ApprovalReview,
) -> Option<ReviewOutcome> {
    if state.stage != RegistrationStage::Provisioned {
        return None;
    }
    state.approval?.review(review)
}

pub(crate) fn registration_pipeline_provisioned_account(
    state: &RegistrationPipelineModelState,
) -> LocalAccount {
    let validated = registration_pipeline_composing(state)
        .validate()
        .expect("validated registration");
    assert!(validated.has_event(&RegistrationEvent::IntentValidated));
    let intent = validated.state;
    let provisioned = intent
        .register(
            registration_pipeline_fixture_account_id(),
            registration_pipeline_fixture_keys(),
        )
        .provision("2026-01-01T00:00:00.000Z".to_owned());
    assert!(provisioned.has_event(&RegistrationEvent::AccountProvisioned));
    provisioned.state
}

impl Model for RegistrationPipelineModel {
    type State = RegistrationPipelineModelState;
    type Action = RegistrationPipelineAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();

        for username in [
            TextFieldInput::Missing,
            TextFieldInput::Blank,
            TextFieldInput::Invalid,
            TextFieldInput::Valid,
        ] {
            for email in [
                TextFieldInput::Missing,
                TextFieldInput::Blank,
                TextFieldInput::Invalid,
                TextFieldInput::Valid,
            ] {
                for password_present in [false, true] {
                    for agreement in [false, true] {
                        for username_taken in [false, true] {
                            for email_taken in [false, true] {
                                for mode in RegistrationsMode::ALL {
                                    for ip in IpSignUpDecision::ALL {
                                        states.push(RegistrationPipelineModelState {
                                            stage: RegistrationStage::Composing,
                                            username,
                                            email,
                                            password_present,
                                            agreement,
                                            username_taken,
                                            email_taken,
                                            mode,
                                            ip,
                                            approval: None,
                                            reviewed: false,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        states
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        match state.stage {
            RegistrationStage::Composing => {
                actions.extend([
                    RegistrationPipelineAction::CycleUsername,
                    RegistrationPipelineAction::CycleEmail,
                    RegistrationPipelineAction::TogglePassword,
                    RegistrationPipelineAction::ToggleAgreement,
                    RegistrationPipelineAction::ToggleUsernameTaken,
                    RegistrationPipelineAction::ToggleEmailTaken,
                    RegistrationPipelineAction::Validate,
                ]);
            }
            RegistrationStage::Validated => {
                if registration_pipeline_register_allowed(state) {
                    actions.push(RegistrationPipelineAction::Register);
                }
            }
            RegistrationStage::Registered => {
                actions.push(RegistrationPipelineAction::Provision);
            }
            RegistrationStage::Provisioned => {
                if registration_pipeline_review(state, ApprovalReview::Approve).is_some() {
                    actions.push(RegistrationPipelineAction::Approve);
                }
                if registration_pipeline_review(state, ApprovalReview::Reject).is_some() {
                    actions.push(RegistrationPipelineAction::Reject);
                }
            }
            RegistrationStage::Refused
            | RegistrationStage::ValidationFailed
            | RegistrationStage::Rejected => {}
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            RegistrationPipelineAction::CycleUsername => {
                if state.stage != RegistrationStage::Composing {
                    return None;
                }
                next.username = Self::cycle_field(next.username);
            }
            RegistrationPipelineAction::CycleEmail => {
                if state.stage != RegistrationStage::Composing {
                    return None;
                }
                next.email = Self::cycle_field(next.email);
            }
            RegistrationPipelineAction::TogglePassword => {
                if state.stage != RegistrationStage::Composing {
                    return None;
                }
                next.password_present = !next.password_present;
            }
            RegistrationPipelineAction::ToggleAgreement => {
                if state.stage != RegistrationStage::Composing {
                    return None;
                }
                next.agreement = !next.agreement;
            }
            RegistrationPipelineAction::ToggleUsernameTaken => {
                if state.stage != RegistrationStage::Composing {
                    return None;
                }
                next.username_taken = !next.username_taken;
            }
            RegistrationPipelineAction::ToggleEmailTaken => {
                if state.stage != RegistrationStage::Composing {
                    return None;
                }
                next.email_taken = !next.email_taken;
            }
            RegistrationPipelineAction::Validate => {
                if state.stage != RegistrationStage::Composing {
                    return None;
                }
                next.stage = apply_registration_pipeline_validate(state);
            }
            RegistrationPipelineAction::Register => {
                if !registration_pipeline_register_allowed(state) {
                    return None;
                }
                next.stage = RegistrationStage::Registered;
                next.approval = registration_pipeline_sign_up_approval(state);
            }
            RegistrationPipelineAction::Provision => {
                if state.stage != RegistrationStage::Registered {
                    return None;
                }
                next.stage = RegistrationStage::Provisioned;
            }
            RegistrationPipelineAction::Approve => {
                registration_pipeline_review(state, ApprovalReview::Approve)?;
                next.approval = Some(AccountApproval::Approved);
                next.reviewed = true;
            }
            RegistrationPipelineAction::Reject => {
                registration_pipeline_review(state, ApprovalReview::Reject)?;
                next.stage = RegistrationStage::Rejected;
                next.approval = None;
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "validation_failed_matches_domain_rules",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage != RegistrationStage::ValidationFailed
                        || !Self::expected_validation_errors(state).is_empty()
                },
            ),
            Property::always(
                "validated_inputs_have_no_validation_errors",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage != RegistrationStage::Validated
                        || Self::expected_validation_errors(state).is_empty()
                },
            ),
            Property::always(
                "registered_implies_unique_username_and_email",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage != RegistrationStage::Registered
                        && state.stage != RegistrationStage::Provisioned
                        || (!state.username_taken && !state.email_taken)
                },
            ),
            Property::always(
                "registered_implies_validated_inputs",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage != RegistrationStage::Registered
                        && state.stage != RegistrationStage::Provisioned
                        || Self::expected_validation_errors(state).is_empty()
                },
            ),
            Property::always(
                "provisioned_account_preserves_identity",
                |_, state: &RegistrationPipelineModelState| {
                    if state.stage != RegistrationStage::Provisioned {
                        return true;
                    }
                    let account = Self::provisioned_account(state);
                    account.username() == "alice" && account.access_email() == "alice@example.com"
                },
            ),
            Property::always(
                "provisioned_account_uses_fixture_keys",
                |_, state: &RegistrationPipelineModelState| {
                    if state.stage != RegistrationStage::Provisioned {
                        return true;
                    }
                    let account = Self::provisioned_account(state);
                    account.public_key_pem() == "pem-fixture"
                        && account.private_key_jwk() == r#"{"kty":"RSA"}"#
                },
            ),
            Property::always(
                "provisioned_account_has_active_defaults",
                |_, state: &RegistrationPipelineModelState| {
                    if state.stage != RegistrationStage::Provisioned {
                        return true;
                    }
                    let account = Self::provisioned_account(state);
                    !account.is_locked()
                        && account.default_visibility() == Visibility::Public
                        && account.default_quote_policy() == QuoteApprovalPolicy::Public
                },
            ),
            Property::always(
                "provisioned_display_name_matches_username",
                |_, state: &RegistrationPipelineModelState| {
                    if state.stage != RegistrationStage::Provisioned {
                        return true;
                    }
                    let account = Self::provisioned_account(state);
                    account.display_name() == account.username()
                },
            ),
            Property::always(
                "refused_only_for_a_blocked_address",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage != RegistrationStage::Refused
                        || state.ip == IpSignUpDecision::Blocked
                },
            ),
            Property::always(
                "blocked_address_never_creates_an_account",
                |_, state: &RegistrationPipelineModelState| {
                    state.ip != IpSignUpDecision::Blocked
                        || !matches!(
                            state.stage,
                            RegistrationStage::Registered
                                | RegistrationStage::Provisioned
                                | RegistrationStage::Rejected
                        )
                },
            ),
            Property::always(
                "held_sign_up_is_created_pending",
                |_, state: &RegistrationPipelineModelState| {
                    let needs_approval = state.mode == RegistrationsMode::Approved
                        || state.ip == IpSignUpDecision::RequiresApproval;
                    state.stage != RegistrationStage::Registered
                        || state.approval
                            == Some(if needs_approval {
                                AccountApproval::Pending
                            } else {
                                AccountApproval::Approved
                            })
                },
            ),
            Property::always(
                "pending_account_has_no_api_access",
                |_, state: &RegistrationPipelineModelState| {
                    state.approval != Some(AccountApproval::Pending)
                        || !registration_pipeline_api_access(state)
                },
            ),
            Property::always(
                "api_access_needs_open_sign_up_or_review",
                |_, state: &RegistrationPipelineModelState| {
                    !registration_pipeline_api_access(state)
                        || state.reviewed
                        || (state.mode == RegistrationsMode::Open
                            && state.ip == IpSignUpDecision::Open)
                },
            ),
            Property::always(
                "rejected_account_is_gone",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage != RegistrationStage::Rejected
                        || (state.approval.is_none() && !registration_pipeline_api_access(state))
                },
            ),
            Property::sometimes(
                "pending_account_reachable",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage == RegistrationStage::Provisioned
                        && state.approval == Some(AccountApproval::Pending)
                },
            ),
            Property::sometimes(
                "approved_after_review_reachable",
                |_, state: &RegistrationPipelineModelState| {
                    state.reviewed && registration_pipeline_api_access(state)
                },
            ),
            Property::sometimes(
                "rejection_reachable",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage == RegistrationStage::Rejected
                },
            ),
            Property::sometimes(
                "refusal_reachable",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage == RegistrationStage::Refused
                },
            ),
            Property::sometimes(
                "provisioned_account_reachable",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage == RegistrationStage::Provisioned
                },
            ),
            Property::sometimes(
                "validation_failure_reachable",
                |_, state: &RegistrationPipelineModelState| {
                    state.stage == RegistrationStage::ValidationFailed
                },
            ),
        ]
    }
}

pub(crate) fn check_registration_pipeline_model() {
    RegistrationPipelineModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_registration_pipeline_model;

    #[test]
    fn registration_pipeline_model_holds() {
        check_registration_pipeline_model();
    }
}
