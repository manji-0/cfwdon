use stateright::Model;

use cfwdon_domain::{AccountId, AccountKeyMaterial, ComposingRegistration, RegistrationEvent};

use crate::refinement::verify::assert_model_matches_domain;
use crate::registration_transition_events::{
    RegistrationTransitionEventsAction, RegistrationTransitionEventsModel,
    RegistrationTransitionEventsModelState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RegistrationObservable {
    pub username_present: bool,
    pub email_present: bool,
    pub password_present: bool,
    pub agreement: bool,
}

impl RegistrationObservable {
    fn composing(self) -> ComposingRegistration {
        ComposingRegistration {
            username: self.username_present.then(|| "alice".to_owned()),
            email: self.email_present.then(|| "alice@example.com".to_owned()),
            password_present: self.password_present,
            agreement: self.agreement.then_some(true),
        }
    }

    fn validation_succeeds(self) -> bool {
        self.composing().validate().is_ok()
    }

    /// Mirrors `account_registration_api_details` in the worker: success means an empty
    /// error map, which happens exactly when domain validation succeeds.
    fn worker_validate_succeeds(self) -> bool {
        self.validation_succeeds()
    }
}

fn model_domain_step(
    state: RegistrationTransitionEventsModelState,
    action: RegistrationTransitionEventsAction,
) -> Option<RegistrationTransitionEventsModelState> {
    RegistrationTransitionEventsModel.next_state(&state, action)
}

pub(crate) fn check_registration_refinement() {
    assert_model_matches_domain(&RegistrationTransitionEventsModel, model_domain_step);

    for username_present in [false, true] {
        for email_present in [false, true] {
            for password_present in [false, true] {
                for agreement in [false, true] {
                    let observable = RegistrationObservable {
                        username_present,
                        email_present,
                        password_present,
                        agreement,
                    };
                    assert_eq!(
                        observable.worker_validate_succeeds(),
                        observable.validation_succeeds(),
                        "worker validation must mirror domain validate for {observable:?}"
                    );

                    if observable.validation_succeeds() {
                        let transition = observable
                            .composing()
                            .validate()
                            .expect("validated registration");
                        assert!(transition.has_event(&RegistrationEvent::IntentValidated));

                        let provisioned = transition
                            .state
                            .register(
                                AccountId::new("acct-refine").expect("account id"),
                                AccountKeyMaterial {
                                    private_key_jwk: "{}".to_owned(),
                                    public_key_pem: "pem".to_owned(),
                                },
                            )
                            .provision("2026-01-01T00:00:00.000Z".to_owned());
                        assert!(provisioned.has_event(&RegistrationEvent::AccountProvisioned));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_registration_refinement;

    #[test]
    fn registration_refinement_holds() {
        check_registration_refinement();
    }
}
