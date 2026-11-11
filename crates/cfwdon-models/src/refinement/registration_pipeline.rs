use cfwdon_domain::{
    AccountApproval, ApprovalReview, IpSignUpDecision, RegistrationEvent, RegistrationsMode,
    ReviewOutcome, finalize_registration_validation, sign_up_approval,
};

use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use crate::registration_pipeline::{
    RegistrationPipelineAction, RegistrationPipelineModel, RegistrationPipelineModelState,
    RegistrationStage, TextFieldInput, apply_registration_pipeline_validate,
    registration_pipeline_api_access, registration_pipeline_composing,
    registration_pipeline_finalize_allowed, registration_pipeline_provisioned_account,
    registration_pipeline_register_allowed, registration_pipeline_review,
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

/// Mirrors `create_account_placeholder_response`: `sign_up_approval` on the
/// configured mode and the client's IP decision, before the form is read.
fn worker_sign_up_approval(state: RegistrationPipelineModelState) -> Option<AccountApproval> {
    sign_up_approval(state.mode, state.ip).ok()
}

/// Mirrors `review_account_response`: the domain review of the account's
/// approval, then a write conditional on the pending row.
fn worker_review(
    state: RegistrationPipelineModelState,
    review: ApprovalReview,
) -> Option<ReviewOutcome> {
    if state.stage != RegistrationStage::Provisioned {
        return None;
    }
    state.approval?.review(review)
}

/// Mirrors `oauth_access_token_auth_from_joined_row` and the Auth0 sign-in:
/// an account with a pending row decodes as no account.
fn worker_api_access(state: RegistrationPipelineModelState) -> bool {
    state.stage == RegistrationStage::Provisioned
        && state
            .approval
            .is_some_and(AccountApproval::grants_api_access)
}

fn worker_allows(
    state: RegistrationPipelineModelState,
    action: RegistrationPipelineAction,
) -> bool {
    match action {
        RegistrationPipelineAction::Validate => state.stage == RegistrationStage::Composing,
        RegistrationPipelineAction::Register => registration_pipeline_register_allowed(&state),
        RegistrationPipelineAction::Provision => state.stage == RegistrationStage::Registered,
        RegistrationPipelineAction::Approve => {
            worker_review(state, ApprovalReview::Approve).is_some()
        }
        RegistrationPipelineAction::Reject => {
            worker_review(state, ApprovalReview::Reject).is_some()
        }
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
            state.stage = if worker_sign_up_approval(state).is_none() {
                RegistrationStage::Refused
            } else if worker_validate_succeeds(state) {
                RegistrationStage::Validated
            } else {
                RegistrationStage::ValidationFailed
            };
        }
        RegistrationPipelineAction::Register => {
            state.stage = RegistrationStage::Registered;
            state.approval = worker_sign_up_approval(state);
        }
        RegistrationPipelineAction::Provision => {
            state.stage = RegistrationStage::Provisioned;
        }
        RegistrationPipelineAction::Approve => {
            state.approval = Some(AccountApproval::Approved);
            state.reviewed = true;
        }
        RegistrationPipelineAction::Reject => {
            state.stage = RegistrationStage::Rejected;
            state.approval = None;
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
        let Some(approval) = worker_sign_up_approval(state) else {
            continue;
        };
        if registration_pipeline_validation_errors(&state).is_empty() {
            let mut validated = state;
            validated.stage = RegistrationStage::Validated;
            states.push(validated);

            if registration_pipeline_register_allowed(&validated) {
                let mut registered = validated;
                registered.stage = RegistrationStage::Registered;
                registered.approval = Some(approval);
                states.push(registered);

                let mut provisioned = registered;
                provisioned.stage = RegistrationStage::Provisioned;
                states.push(provisioned);
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
            RegistrationStage::Provisioned => vec![
                RegistrationPipelineAction::Approve,
                RegistrationPipelineAction::Reject,
            ],
            RegistrationStage::Refused
            | RegistrationStage::ValidationFailed
            | RegistrationStage::Rejected => Vec::new(),
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
            worker_sign_up_approval(state).is_some() && worker_validate_succeeds(state),
            "validate stage for {state:?}"
        );
        assert_eq!(
            worker_stage == RegistrationStage::Refused,
            state.ip == IpSignUpDecision::Blocked,
            "refusal for {state:?}"
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
            state.stage == RegistrationStage::Validated
                && worker_sign_up_approval(state).is_some()
                && worker_register_succeeds(state),
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

    for state in worker_states() {
        assert_eq!(
            worker_api_access(state),
            registration_pipeline_api_access(&state),
            "API access for {state:?}"
        );
        for review in [ApprovalReview::Approve, ApprovalReview::Reject] {
            assert_eq!(
                worker_review(state, review),
                registration_pipeline_review(&state, review),
                "{review:?} for {state:?}"
            );
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
        mode: RegistrationsMode::Open,
        ip: IpSignUpDecision::Open,
        approval: None,
        reviewed: false,
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
        mode: RegistrationsMode::Open,
        ip: IpSignUpDecision::Open,
        approval: None,
        reviewed: false,
    };
    assert!(!registration_pipeline_register_allowed(&taken));

    let held = RegistrationPipelineModelState {
        stage: RegistrationStage::Provisioned,
        username: TextFieldInput::Valid,
        email: TextFieldInput::Valid,
        password_present: true,
        agreement: true,
        username_taken: false,
        email_taken: false,
        mode: RegistrationsMode::Open,
        ip: IpSignUpDecision::RequiresApproval,
        approval: Some(AccountApproval::Pending),
        reviewed: false,
    };
    assert!(!worker_api_access(held));
    let approved = worker_effect(held, RegistrationPipelineAction::Approve);
    assert!(worker_api_access(approved));
    assert!(!worker_allows(approved, RegistrationPipelineAction::Reject));
    let rejected = worker_effect(held, RegistrationPipelineAction::Reject);
    assert_eq!(rejected.stage, RegistrationStage::Rejected);
    assert!(!worker_api_access(rejected));
}

#[cfg(test)]
mod tests {
    use super::check_registration_pipeline_refinement;

    #[test]
    fn registration_pipeline_refinement_holds() {
        check_registration_pipeline_refinement();
    }
}
