use cfwdon_domain::AccessEmail;

use crate::access_provision::{
    AccessProvisionAction, AccessProvisionModel, AccessProvisionModelState, EmailInput,
    ProvisionStage, access_provision_composing, access_provision_email_value,
    access_provision_provisioned_account, access_provision_resolution_succeeds,
    access_provision_resolved_intent, access_provision_sanitized_local_part,
    apply_access_provision_resolve,
};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use stateright::Model;

fn model_domain_step(
    state: AccessProvisionModelState,
    action: AccessProvisionAction,
) -> Option<AccessProvisionModelState> {
    AccessProvisionModel.next_state(&state, action)
}

/// Mirrors `resolve_local_account` resolution gate before account insert.
fn worker_resolve_succeeds(state: AccessProvisionModelState) -> bool {
    access_provision_resolution_succeeds(&state)
}

fn worker_allows(state: AccessProvisionModelState, action: AccessProvisionAction) -> bool {
    match action {
        AccessProvisionAction::Resolve => state.stage == ProvisionStage::Composing,
        AccessProvisionAction::Register => state.stage == ProvisionStage::Resolved,
        AccessProvisionAction::Provision => state.stage == ProvisionStage::Registered,
        _ => false,
    }
}

fn worker_effect(
    mut state: AccessProvisionModelState,
    action: AccessProvisionAction,
) -> AccessProvisionModelState {
    if !worker_allows(state, action) {
        return state;
    }

    match action {
        AccessProvisionAction::Resolve => {
            state.stage = apply_access_provision_resolve(&state);
        }
        AccessProvisionAction::Register => {
            state.stage = ProvisionStage::Registered;
        }
        AccessProvisionAction::Provision => {
            state.stage = ProvisionStage::Provisioned;
        }
        _ => {}
    }

    state
}

fn domain_step(
    state: AccessProvisionModelState,
    action: AccessProvisionAction,
) -> AccessProvisionModelState {
    model_domain_step(state, action).unwrap_or(state)
}

fn worker_states() -> Vec<AccessProvisionModelState> {
    let mut states = AccessProvisionModel.init_states();
    for state in AccessProvisionModel.init_states() {
        if access_provision_resolution_succeeds(&state) {
            let mut resolved = state;
            resolved.stage = ProvisionStage::Resolved;
            states.push(resolved);

            let mut registered = resolved;
            registered.stage = ProvisionStage::Registered;
            states.push(registered);
        }
    }
    states
}

pub(crate) fn check_access_provision_refinement() {
    assert_model_matches_domain(&AccessProvisionModel, model_domain_step);

    assert_worker_refinement(
        worker_states(),
        |state| match state.stage {
            ProvisionStage::Composing => vec![AccessProvisionAction::Resolve],
            ProvisionStage::Resolved => vec![AccessProvisionAction::Register],
            ProvisionStage::Registered => vec![AccessProvisionAction::Provision],
            ProvisionStage::ResolutionFailed | ProvisionStage::Provisioned => Vec::new(),
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    for state in AccessProvisionModel.init_states() {
        let composing = access_provision_composing(&state);
        let domain_ok = composing.clone().resolve(state.base_username_taken).is_ok();
        assert_eq!(
            access_provision_resolution_succeeds(&state),
            domain_ok,
            "resolution parity for {state:?}"
        );

        assert_eq!(
            worker_resolve_succeeds(state),
            access_provision_resolution_succeeds(&state),
            "worker resolution parity for {state:?}"
        );

        let worker_stage = apply_access_provision_resolve(&state);
        assert_eq!(
            worker_stage == ProvisionStage::Resolved,
            worker_resolve_succeeds(state),
            "resolve stage for {state:?}"
        );

        if worker_resolve_succeeds(state) {
            let intent = access_provision_resolved_intent(&state);
            let email = AccessEmail::parse(&access_provision_email_value(state.email))
                .expect("valid email");
            assert_eq!(
                intent.email.as_str(),
                access_provision_email_value(state.email)
                    .trim()
                    .to_ascii_lowercase()
            );

            if state.base_username_taken {
                let base = access_provision_sanitized_local_part(&email);
                assert!(
                    intent.username.as_str().starts_with(&format!("{base}_")),
                    "suffix username for {state:?}"
                );
            } else {
                assert_eq!(
                    intent.username.as_str(),
                    access_provision_sanitized_local_part(&email),
                    "sanitized username for {state:?}"
                );
            }

            let account = access_provision_provisioned_account(&state);
            assert_eq!(account.username(), intent.username.as_str());
            assert_eq!(
                account.access_email(),
                access_provision_email_value(state.email)
                    .trim()
                    .to_ascii_lowercase()
            );
        }
    }

    let invalid = AccessProvisionModelState {
        stage: ProvisionStage::Composing,
        email: EmailInput::Invalid,
        base_username_taken: false,
    };
    assert_eq!(
        apply_access_provision_resolve(&invalid),
        ProvisionStage::ResolutionFailed
    );
}

#[cfg(test)]
mod tests {
    use super::check_access_provision_refinement;

    #[test]
    fn access_provision_refinement_holds() {
        check_access_provision_refinement();
    }
}
