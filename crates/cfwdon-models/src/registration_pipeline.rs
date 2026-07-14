use cfwdon_domain::{
    AccountId, AccountKeyMaterial, ComposingRegistration, LocalAccount, QuoteApprovalPolicy,
    RegistrationFieldIssue, RegistrationValidationErrors, Visibility,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RegistrationPipelineModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RegistrationStage {
    Composing,
    Validated,
    ValidationFailed,
    Registered,
    Provisioned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TextFieldInput {
    Missing,
    Blank,
    Invalid,
    Valid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RegistrationPipelineModelState {
    stage: RegistrationStage,
    username: TextFieldInput,
    email: TextFieldInput,
    password_present: bool,
    agreement: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RegistrationPipelineAction {
    CycleUsername,
    CycleEmail,
    TogglePassword,
    ToggleAgreement,
    Validate,
    Register,
    Provision,
}

impl RegistrationPipelineModel {
    fn field_value(input: TextFieldInput, kind: FieldKind) -> Option<String> {
        match (input, kind) {
            (TextFieldInput::Missing, _) => None,
            (TextFieldInput::Blank, _) => Some(String::new()),
            (TextFieldInput::Invalid, FieldKind::Username) => Some("bad name!".to_owned()),
            (TextFieldInput::Invalid, FieldKind::Email) => Some("not-an-email".to_owned()),
            (TextFieldInput::Valid, FieldKind::Username) => Some("alice".to_owned()),
            (TextFieldInput::Valid, FieldKind::Email) => Some("alice@example.com".to_owned()),
        }
    }

    fn composing_registration(state: &RegistrationPipelineModelState) -> ComposingRegistration {
        ComposingRegistration {
            username: Self::field_value(state.username, FieldKind::Username),
            email: Self::field_value(state.email, FieldKind::Email),
            password_present: state.password_present,
            agreement: state.agreement.then_some(true),
        }
    }

    fn expected_validation_errors(
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

    fn fixture_keys() -> AccountKeyMaterial {
        AccountKeyMaterial {
            private_key_jwk: r#"{"kty":"RSA"}"#.to_owned(),
            public_key_pem: "pem-fixture".to_owned(),
        }
    }

    fn fixture_account_id() -> AccountId {
        AccountId::new("acct-model").expect("fixture account id")
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
        let intent = Self::composing_registration(state)
            .validate()
            .expect("validated registration")
            .state;
        intent
            .register(Self::fixture_account_id(), Self::fixture_keys())
            .provision("2026-01-01T00:00:00.000Z".to_owned())
            .state
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FieldKind {
    Username,
    Email,
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
                        states.push(RegistrationPipelineModelState {
                            stage: RegistrationStage::Composing,
                            username,
                            email,
                            password_present,
                            agreement,
                        });
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
                    RegistrationPipelineAction::Validate,
                ]);
            }
            RegistrationStage::Validated => {
                actions.push(RegistrationPipelineAction::Register);
            }
            RegistrationStage::Registered => {
                actions.push(RegistrationPipelineAction::Provision);
            }
            RegistrationStage::ValidationFailed | RegistrationStage::Provisioned => {}
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
            RegistrationPipelineAction::Validate => {
                if state.stage != RegistrationStage::Composing {
                    return None;
                }
                if Self::expected_validation_errors(state).is_empty() {
                    next.stage = RegistrationStage::Validated;
                } else {
                    next.stage = RegistrationStage::ValidationFailed;
                }
            }
            RegistrationPipelineAction::Register => {
                if state.stage != RegistrationStage::Validated {
                    return None;
                }
                next.stage = RegistrationStage::Registered;
            }
            RegistrationPipelineAction::Provision => {
                if state.stage != RegistrationStage::Registered {
                    return None;
                }
                next.stage = RegistrationStage::Provisioned;
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
