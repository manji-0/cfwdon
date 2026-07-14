use cfwdon_domain::{
    AccessEmail, AccountId, AccountKeyMaterial, ComposingAccessProvision, LocalAccount,
    QuoteApprovalPolicy, Username, Visibility,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct AccessProvisionModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ProvisionStage {
    Composing,
    Resolved,
    ResolutionFailed,
    Registered,
    Provisioned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EmailInput {
    Blank,
    Invalid,
    Standard,
    DottedLocal,
    PlusLocal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AccessProvisionModelState {
    stage: ProvisionStage,
    email: EmailInput,
    base_username_taken: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AccessProvisionAction {
    CycleEmail,
    ToggleBaseUsernameTaken,
    Resolve,
    Register,
    Provision,
}

impl AccessProvisionModel {
    fn email_value(input: EmailInput) -> String {
        match input {
            EmailInput::Blank => String::new(),
            EmailInput::Invalid => "not-an-email".to_owned(),
            EmailInput::Standard => "alice@example.com".to_owned(),
            EmailInput::DottedLocal => "alice.bob@example.com".to_owned(),
            EmailInput::PlusLocal => "alice+foo@example.com".to_owned(),
        }
    }

    fn composing_provision(state: &AccessProvisionModelState) -> ComposingAccessProvision {
        ComposingAccessProvision {
            email: Self::email_value(state.email),
        }
    }

    fn resolution_succeeds(state: &AccessProvisionModelState) -> bool {
        AccessEmail::parse(&Self::email_value(state.email)).is_ok()
    }

    fn sanitized_local_part(email: &AccessEmail) -> String {
        Username::derive_from_email(email, false).into_inner()
    }

    fn resolved_intent(state: &AccessProvisionModelState) -> cfwdon_domain::AccessProvisionIntent {
        Self::composing_provision(state)
            .resolve(state.base_username_taken)
            .expect("resolved access provision")
    }

    fn fixture_keys() -> AccountKeyMaterial {
        AccountKeyMaterial {
            private_key_jwk: r#"{"kty":"RSA"}"#.to_owned(),
            public_key_pem: "pem-fixture".to_owned(),
        }
    }

    fn fixture_account_id() -> AccountId {
        AccountId::new("acct-access").expect("fixture account id")
    }

    fn cycle_email(input: EmailInput) -> EmailInput {
        match input {
            EmailInput::Blank => EmailInput::Invalid,
            EmailInput::Invalid => EmailInput::Standard,
            EmailInput::Standard => EmailInput::DottedLocal,
            EmailInput::DottedLocal => EmailInput::PlusLocal,
            EmailInput::PlusLocal => EmailInput::Blank,
        }
    }

    fn provisioned_account(state: &AccessProvisionModelState) -> LocalAccount {
        Self::resolved_intent(state)
            .register(Self::fixture_account_id(), Self::fixture_keys())
            .provision("2026-01-01T00:00:00.000Z".to_owned())
            .state
    }
}

impl Model for AccessProvisionModel {
    type State = AccessProvisionModelState;
    type Action = AccessProvisionAction;

    fn init_states(&self) -> Vec<Self::State> {
        let mut states = Vec::new();

        for email in [
            EmailInput::Blank,
            EmailInput::Invalid,
            EmailInput::Standard,
            EmailInput::DottedLocal,
            EmailInput::PlusLocal,
        ] {
            for base_username_taken in [false, true] {
                states.push(AccessProvisionModelState {
                    stage: ProvisionStage::Composing,
                    email,
                    base_username_taken,
                });
            }
        }

        states
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        match state.stage {
            ProvisionStage::Composing => {
                actions.push(AccessProvisionAction::CycleEmail);
                actions.push(AccessProvisionAction::ToggleBaseUsernameTaken);
                actions.push(AccessProvisionAction::Resolve);
            }
            ProvisionStage::Resolved => {
                actions.push(AccessProvisionAction::Register);
            }
            ProvisionStage::Registered => {
                actions.push(AccessProvisionAction::Provision);
            }
            ProvisionStage::ResolutionFailed | ProvisionStage::Provisioned => {}
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            AccessProvisionAction::CycleEmail => {
                if state.stage != ProvisionStage::Composing {
                    return None;
                }
                next.email = Self::cycle_email(next.email);
            }
            AccessProvisionAction::ToggleBaseUsernameTaken => {
                if state.stage != ProvisionStage::Composing {
                    return None;
                }
                next.base_username_taken = !next.base_username_taken;
            }
            AccessProvisionAction::Resolve => {
                if state.stage != ProvisionStage::Composing {
                    return None;
                }
                if Self::resolution_succeeds(state) {
                    next.stage = ProvisionStage::Resolved;
                } else {
                    next.stage = ProvisionStage::ResolutionFailed;
                }
            }
            AccessProvisionAction::Register => {
                if state.stage != ProvisionStage::Resolved {
                    return None;
                }
                next.stage = ProvisionStage::Registered;
            }
            AccessProvisionAction::Provision => {
                if state.stage != ProvisionStage::Registered {
                    return None;
                }
                next.stage = ProvisionStage::Provisioned;
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "resolution_failed_only_for_invalid_email",
                |_, state: &AccessProvisionModelState| {
                    state.stage != ProvisionStage::ResolutionFailed
                        || !Self::resolution_succeeds(state)
                },
            ),
            Property::always(
                "resolved_inputs_have_valid_email",
                |_, state: &AccessProvisionModelState| {
                    state.stage != ProvisionStage::Resolved || Self::resolution_succeeds(state)
                },
            ),
            Property::always(
                "resolved_email_matches_input",
                |_, state: &AccessProvisionModelState| {
                    if !matches!(
                        state.stage,
                        ProvisionStage::Resolved
                            | ProvisionStage::Registered
                            | ProvisionStage::Provisioned
                    ) {
                        return true;
                    }
                    let intent = Self::resolved_intent(state);
                    intent.email.as_str()
                        == Self::email_value(state.email).trim().to_ascii_lowercase()
                },
            ),
            Property::always(
                "available_username_uses_sanitized_local_part",
                |_, state: &AccessProvisionModelState| {
                    if !matches!(
                        state.stage,
                        ProvisionStage::Resolved
                            | ProvisionStage::Registered
                            | ProvisionStage::Provisioned
                    ) || state.base_username_taken
                    {
                        return true;
                    }
                    let intent = Self::resolved_intent(state);
                    let email =
                        AccessEmail::parse(&Self::email_value(state.email)).expect("valid email");
                    intent.username.as_str() == Self::sanitized_local_part(&email)
                },
            ),
            Property::always(
                "taken_username_appends_suffix",
                |_, state: &AccessProvisionModelState| {
                    if !matches!(
                        state.stage,
                        ProvisionStage::Resolved
                            | ProvisionStage::Registered
                            | ProvisionStage::Provisioned
                    ) || !state.base_username_taken
                    {
                        return true;
                    }
                    let intent = Self::resolved_intent(state);
                    let email =
                        AccessEmail::parse(&Self::email_value(state.email)).expect("valid email");
                    let base = Self::sanitized_local_part(&email);
                    intent.username.as_str().starts_with(&format!("{base}-"))
                },
            ),
            Property::always(
                "provisioned_account_preserves_access_email",
                |_, state: &AccessProvisionModelState| {
                    if state.stage != ProvisionStage::Provisioned {
                        return true;
                    }
                    let account = Self::provisioned_account(state);
                    account.access_email()
                        == Self::email_value(state.email).trim().to_ascii_lowercase()
                },
            ),
            Property::always(
                "provisioned_account_matches_resolved_username",
                |_, state: &AccessProvisionModelState| {
                    if state.stage != ProvisionStage::Provisioned {
                        return true;
                    }
                    let account = Self::provisioned_account(state);
                    account.username() == Self::resolved_intent(state).username.as_str()
                },
            ),
            Property::always(
                "provisioned_account_has_active_defaults",
                |_, state: &AccessProvisionModelState| {
                    if state.stage != ProvisionStage::Provisioned {
                        return true;
                    }
                    let account = Self::provisioned_account(state);
                    !account.is_locked()
                        && account.default_visibility() == Visibility::Public
                        && account.default_quote_policy() == QuoteApprovalPolicy::Public
                },
            ),
            Property::sometimes(
                "provisioned_account_reachable",
                |_, state: &AccessProvisionModelState| state.stage == ProvisionStage::Provisioned,
            ),
            Property::sometimes(
                "resolution_failure_reachable",
                |_, state: &AccessProvisionModelState| {
                    state.stage == ProvisionStage::ResolutionFailed
                },
            ),
        ]
    }
}

pub(crate) fn check_access_provision_model() {
    AccessProvisionModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_access_provision_model;

    #[test]
    fn access_provision_model_holds() {
        check_access_provision_model();
    }
}
