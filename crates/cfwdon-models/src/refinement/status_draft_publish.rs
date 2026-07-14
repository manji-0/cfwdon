use cfwdon_domain::{QuoteApprovalPolicy, StatusDraftError};

use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use crate::status_draft_publish::{
    PublishStage, StatusDraftPublishAction, StatusDraftPublishModel, StatusDraftPublishModelState,
    apply_status_draft_publish_publish, apply_status_draft_publish_validate,
    status_draft_publish_composing, status_draft_publish_quoted_status_id,
    status_draft_publish_validation_error,
};
use stateright::Model;

fn model_domain_step(
    state: StatusDraftPublishModelState,
    action: StatusDraftPublishAction,
) -> Option<StatusDraftPublishModelState> {
    StatusDraftPublishModel.next_state(&state, action)
}

/// Mirrors `parse_status_draft` validation gate.
fn worker_validate_succeeds(state: StatusDraftPublishModelState) -> bool {
    status_draft_publish_composing(&state)
        .validate(status_draft_publish_quoted_status_id(&state))
        .is_ok()
}

fn worker_allows(state: StatusDraftPublishModelState, action: StatusDraftPublishAction) -> bool {
    match action {
        StatusDraftPublishAction::Validate => state.stage == PublishStage::Composing,
        StatusDraftPublishAction::Publish => state.stage == PublishStage::Validated,
        _ => false,
    }
}

fn worker_effect(
    mut state: StatusDraftPublishModelState,
    action: StatusDraftPublishAction,
) -> StatusDraftPublishModelState {
    if !worker_allows(state, action) {
        return state;
    }

    match action {
        StatusDraftPublishAction::Validate => {
            state.stage = apply_status_draft_publish_validate(&state);
        }
        StatusDraftPublishAction::Publish => {
            if let Some((policy, quote_state)) = apply_status_draft_publish_publish(&state) {
                state.stage = PublishStage::Published;
                state.published_policy = Some(policy);
                state.published_quote_state = Some(quote_state);
            }
        }
        _ => {}
    }

    state
}

fn domain_step(
    state: StatusDraftPublishModelState,
    action: StatusDraftPublishAction,
) -> StatusDraftPublishModelState {
    model_domain_step(state, action).unwrap_or(state)
}

fn worker_states() -> Vec<StatusDraftPublishModelState> {
    let mut states = StatusDraftPublishModel.init_states();
    for mut state in StatusDraftPublishModel.init_states() {
        if worker_validate_succeeds(state) {
            state.stage = PublishStage::Validated;
            states.push(state);
        }
    }
    states
}

pub(crate) fn check_status_draft_publish_refinement() {
    assert_model_matches_domain(&StatusDraftPublishModel, model_domain_step);

    assert_worker_refinement(
        worker_states(),
        |state| match state.stage {
            PublishStage::Composing => vec![StatusDraftPublishAction::Validate],
            PublishStage::Validated => vec![StatusDraftPublishAction::Publish],
            PublishStage::Published | PublishStage::ValidationFailed => Vec::new(),
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    for state in StatusDraftPublishModel.init_states() {
        assert_eq!(
            worker_validate_succeeds(state),
            status_draft_publish_validation_error(&state).is_none(),
            "validation parity for {state:?}"
        );

        let worker_stage = apply_status_draft_publish_validate(&state);
        assert_eq!(
            worker_stage == PublishStage::Validated,
            worker_validate_succeeds(state),
            "validate stage for {state:?}"
        );

        if worker_validate_succeeds(state) {
            let mut validated = state;
            validated.stage = PublishStage::Validated;
            let published = apply_status_draft_publish_publish(&validated)
                .expect("publish intent for valid draft");
            let model_published = domain_step(validated, StatusDraftPublishAction::Publish);
            assert_eq!(
                model_published.published_policy,
                Some(published.0),
                "quote policy for {state:?}"
            );
            assert_eq!(
                model_published.published_quote_state,
                Some(published.1),
                "quote state for {state:?}"
            );
        }
    }

    let invalid = StatusDraftPublishModelState {
        stage: PublishStage::Composing,
        has_text: false,
        media_count: 5,
        has_poll: false,
        has_quote: false,
        visibility: cfwdon_domain::Visibility::Public,
        policy_override: None,
        account_default: QuoteApprovalPolicy::Followers,
        quote_target_local: false,
        published_policy: None,
        published_quote_state: None,
    };
    assert_eq!(
        status_draft_publish_validation_error(&invalid),
        Some(StatusDraftError::TooManyMedia)
    );
    assert_eq!(
        apply_status_draft_publish_validate(&invalid),
        PublishStage::ValidationFailed
    );
    assert!(apply_status_draft_publish_publish(&invalid).is_none());
}

#[cfg(test)]
mod tests {
    use super::check_status_draft_publish_refinement;

    #[test]
    fn status_draft_publish_refinement_holds() {
        check_status_draft_publish_refinement();
    }
}
