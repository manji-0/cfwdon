use cfwdon_domain::{RegistrationEvent, finalize_registration_validation};

use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use crate::registration_pipeline::{
    RegistrationPipelineAction, RegistrationPipelineModel, RegistrationPipelineModelState,
    RegistrationStage, TextFieldInput, apply_registration_pipeline_validate,
    registration_pipeline_composing, registration_pipeline_finalize_allowed,
    registration_pipeline_provisioned_account, registration_pipeline_register_allowed,
    registration_pipeline_uniqueness_facts, registration_pipeline_validation_errors,
};
use stateright::Model;

fn model_domain_step(
    state: RegistrationPipelineModelState,
    action: RegistrationPipelineAction,
) -> Option<RegistrationPipelineModelState> {
    RegistrationPipelineModel.next_state(&state, action)
}

/// Mirrors `account_registration_api_details` success gate.
fn worker_validate_succeeds(state: RegistrationPipelineModelState) -> bool {
    registration_pipeline_validation_errors(&state).is_empty()
}

/// Mirrors account registration handler success gate before insert.
fn worker_register_succeeds(state: RegistrationPipelineModelState) -> bool {
    registration_pipeline_finalize_allowed(&state)
}

fn worker_allows(
    state: RegistrationPipelineModelState,
    action: RegistrationPipelineAction,
) -> bool {
    match action {
        RegistrationPipelineAction::Validate => state.stage == RegistrationStage::Composing,
        RegistrationPipelineAction::Register => registration_pipeline_register_allowed(&state),
        RegistrationPipelineAction::Provision => state.stage == RegistrationStage::Registered,
        _ => false,
    }
}

fn worker_effect(
    mut state: RegistrationPipelineModelState,
    action: RegistrationPipelineAction,
) -> RegistrationPipelineModelState {
    if !worker_allows(state, action) {
        return state;
    }

    match action {
        RegistrationPipelineAction::Validate => {
            state.stage = apply_registration_pipeline_validate(&state);
        }
        RegistrationPipelineAction::Register => {
            state.stage = RegistrationStage::Registered;
        }
        RegistrationPipelineAction::Provision => {
            state.stage = RegistrationStage::Provisioned;
        }
        _ => {}
    }

    state
}

fn domain_step(
    state: RegistrationPipelineModelState,
    action: RegistrationPipelineAction,
) -> RegistrationPipelineModelState {
    model_domain_step(state, action).unwrap_or(state)
}

fn worker_states() -> Vec<RegistrationPipelineModelState> {
    let mut states = RegistrationPipelineModel.init_states();
    for state in RegistrationPipelineModel.init_states() {
        if registration_pipeline_validation_errors(&state).is_empty() {
            let mut validated = state;
            validated.stage = RegistrationStage::Validated;
            states.push(validated);

            if registration_pipeline_register_allowed(&validated) {
                let mut registered = validated;
                registered.stage = RegistrationStage::Registered;
                states.push(registered);
            }
        }
    }
    states
}

pub(crate) fn check_registration_pipeline_refinement() {
    assert_model_matches_domain(&RegistrationPipelineModel, model_domain_step);

    assert_worker_refinement(
        worker_states(),
        |state| match state.stage {
            RegistrationStage::Composing => vec![RegistrationPipelineAction::Validate],
            RegistrationStage::Validated => {
                if registration_pipeline_register_allowed(state) {
                    vec![RegistrationPipelineAction::Register]
                } else {
                    Vec::new()
                }
            }
            RegistrationStage::Registered => vec![RegistrationPipelineAction::Provision],
            RegistrationStage::ValidationFailed | RegistrationStage::Provisioned => Vec::new(),
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    for state in RegistrationPipelineModel.init_states() {
        let composing = registration_pipeline_composing(&state);
        let domain_errors = composing.clone().validate().err().unwrap_or_default();
        assert_eq!(
            registration_pipeline_validation_errors(&state),
            domain_errors,
            "validation errors for {state:?}"
        );

        assert_eq!(
            worker_validate_succeeds(state),
            registration_pipeline_validation_errors(&state).is_empty(),
            "worker validation parity for {state:?}"
        );

        let worker_stage = apply_registration_pipeline_validate(&state);
        assert_eq!(
            worker_stage == RegistrationStage::Validated,
            worker_validate_succeeds(state),
            "validate stage for {state:?}"
        );

        let finalize_ok = finalize_registration_validation(
            composing.clone().validate(),
            registration_pipeline_uniqueness_facts(&state),
        )
        .is_ok();
        assert_eq!(
            worker_register_succeeds(state),
            finalize_ok,
            "register finalize parity for {state:?}"
        );
        assert_eq!(
            registration_pipeline_register_allowed(&state),
            state.stage == RegistrationStage::Validated && worker_register_succeeds(state),
            "register guard for {state:?}"
        );

        if worker_register_succeeds(state) {
            let account = registration_pipeline_provisioned_account(&state);
            assert_eq!(account.username(), "alice");
            assert_eq!(account.access_email(), "alice@example.com");

            let validated = composing.validate().expect("validated registration");
            assert!(validated.has_event(&RegistrationEvent::IntentValidated));
        }
    }

    let invalid = RegistrationPipelineModelState {
        stage: RegistrationStage::Composing,
        username: TextFieldInput::Invalid,
        email: TextFieldInput::Valid,
        password_present: true,
        agreement: true,
        username_taken: false,
        email_taken: false,
    };
    assert_eq!(
        apply_registration_pipeline_validate(&invalid),
        RegistrationStage::ValidationFailed
    );

    let taken = RegistrationPipelineModelState {
        stage: RegistrationStage::Composing,
        username: TextFieldInput::Valid,
        email: TextFieldInput::Valid,
        password_present: true,
        agreement: true,
        username_taken: true,
        email_taken: false,
    };
    assert!(!registration_pipeline_register_allowed(&taken));
}

#[cfg(test)]
mod tests {
    use super::check_registration_pipeline_refinement;

    #[test]
    fn registration_pipeline_refinement_holds() {
        check_registration_pipeline_refinement();
    }
}
