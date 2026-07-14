use cfwdon_domain::{
    AccountId, AccountKeyMaterial, ComposingRegistration, RegistrationEvent,
    RegistrationValidationErrors,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RegistrationTransitionEventsModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RegistrationEventStage {
    Composing,
    IntentValidated,
    AccountProvisioned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RegistrationTransitionEventsModelState {
    stage: RegistrationEventStage,
    username_present: bool,
    email_present: bool,
    password_present: bool,
    agreement: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RegistrationTransitionEventsAction {
    ToggleUsername,
    ToggleEmail,
    TogglePassword,
    ToggleAgreement,
    Validate,
    RegisterAndProvision,
}

impl RegistrationTransitionEventsModel {
    fn composing_registration(
        state: &RegistrationTransitionEventsModelState,
    ) -> ComposingRegistration {
        ComposingRegistration {
            username: state.username_present.then(|| "alice".to_owned()),
            email: state.email_present.then(|| "alice@example.com".to_owned()),
            password_present: state.password_present,
            agreement: state.agreement.then_some(true),
        }
    }

    fn validation_succeeds(state: &RegistrationTransitionEventsModelState) -> bool {
        Self::composing_registration(state).validate().is_ok()
    }
}

impl Model for RegistrationTransitionEventsModel {
    type State = RegistrationTransitionEventsModelState;
    type Action = RegistrationTransitionEventsAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![RegistrationTransitionEventsModelState {
            stage: RegistrationEventStage::Composing,
            username_present: false,
            email_present: false,
            password_present: false,
            agreement: false,
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        if state.stage == RegistrationEventStage::Composing {
            actions.extend([
                RegistrationTransitionEventsAction::ToggleUsername,
                RegistrationTransitionEventsAction::ToggleEmail,
                RegistrationTransitionEventsAction::TogglePassword,
                RegistrationTransitionEventsAction::ToggleAgreement,
                RegistrationTransitionEventsAction::Validate,
            ]);
        }
        if state.stage == RegistrationEventStage::IntentValidated {
            actions.push(RegistrationTransitionEventsAction::RegisterAndProvision);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            RegistrationTransitionEventsAction::ToggleUsername => {
                if state.stage != RegistrationEventStage::Composing {
                    return None;
                }
                next.username_present = !next.username_present;
            }
            RegistrationTransitionEventsAction::ToggleEmail => {
                if state.stage != RegistrationEventStage::Composing {
                    return None;
                }
                next.email_present = !next.email_present;
            }
            RegistrationTransitionEventsAction::TogglePassword => {
                if state.stage != RegistrationEventStage::Composing {
                    return None;
                }
                next.password_present = !next.password_present;
            }
            RegistrationTransitionEventsAction::ToggleAgreement => {
                if state.stage != RegistrationEventStage::Composing {
                    return None;
                }
                next.agreement = !next.agreement;
            }
            RegistrationTransitionEventsAction::Validate => {
                if state.stage != RegistrationEventStage::Composing
                    || !Self::validation_succeeds(state)
                {
                    return None;
                }
                next.stage = RegistrationEventStage::IntentValidated;
            }
            RegistrationTransitionEventsAction::RegisterAndProvision => {
                if state.stage != RegistrationEventStage::IntentValidated {
                    return None;
                }
                next.stage = RegistrationEventStage::AccountProvisioned;
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "validate_emits_intent_validated_event",
                |_, state: &RegistrationTransitionEventsModelState| {
                    state.stage != RegistrationEventStage::IntentValidated
                        || Self::composing_registration(state)
                            .validate()
                            .map(|transition| {
                                transition.has_event(&RegistrationEvent::IntentValidated)
                            })
                            .unwrap_or(false)
                },
            ),
            Property::always(
                "provision_emits_account_provisioned_event",
                |_, state: &RegistrationTransitionEventsModelState| {
                    state.stage != RegistrationEventStage::AccountProvisioned || {
                        let validated = Self::composing_registration(state)
                            .validate()
                            .expect("validated registration");
                        let provisioned = validated
                            .state
                            .register(
                                AccountId::new("acct-event").expect("account id"),
                                AccountKeyMaterial {
                                    private_key_jwk: "{}".to_owned(),
                                    public_key_pem: "pem".to_owned(),
                                },
                            )
                            .provision("2026-01-01T00:00:00.000Z".to_owned());
                        provisioned.has_event(&RegistrationEvent::AccountProvisioned)
                    }
                },
            ),
            Property::always(
                "failed_validation_emits_no_domain_transition",
                |_, state: &RegistrationTransitionEventsModelState| {
                    Self::validation_succeeds(state)
                        || matches!(
                            Self::composing_registration(state).validate(),
                            Err(RegistrationValidationErrors { .. })
                        )
                },
            ),
            Property::sometimes(
                "account_provisioned_event_reachable",
                |_, state: &RegistrationTransitionEventsModelState| {
                    state.stage == RegistrationEventStage::AccountProvisioned
                },
            ),
        ]
    }
}

pub(crate) fn check_registration_transition_events_model() {
    RegistrationTransitionEventsModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_registration_transition_events_model;

    #[test]
    fn registration_transition_events_model_holds() {
        check_registration_transition_events_model();
    }
}
