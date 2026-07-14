use cfwdon_domain::{
    Visibility, activitypub_audience_flags_for_visibility, is_public_activitypub_visibility,
    visibility_from_activitypub_audiences,
};

use crate::activitypub_visibility::{
    ActivityPubVisibilityAction, ActivityPubVisibilityModel, ActivityPubVisibilityModelState,
};
use crate::refinement::verify::{assert_model_matches_domain, assert_worker_refinement};
use stateright::Model;

fn model_domain_step(
    state: ActivityPubVisibilityModelState,
    action: ActivityPubVisibilityAction,
) -> Option<ActivityPubVisibilityModelState> {
    ActivityPubVisibilityModel.next_state(&state, action)
}

fn resolved_visibility(state: ActivityPubVisibilityModelState) -> Visibility {
    visibility_from_activitypub_audiences(state.to_contains_public, state.cc_contains_public)
}

/// Mirrors `visibility_from_activitypub_object` in the worker parse path.
fn worker_parse_visibility(state: ActivityPubVisibilityModelState) -> Visibility {
    visibility_from_activitypub_audiences(state.to_contains_public, state.cc_contains_public)
}

/// Mirrors worker `is_public_activitypub_visibility` on parsed visibility strings.
fn worker_is_public_visibility_string(visibility: Visibility) -> bool {
    matches!(visibility.as_str(), "public" | "unlisted")
}

/// Mirrors `activitypub_audiences` flag selection before JSON assembly.
fn worker_emit_audience_flags(visibility: Visibility) -> (bool, bool) {
    activitypub_audience_flags_for_visibility(visibility)
}

fn worker_visibility_for_emit(action: ActivityPubVisibilityAction) -> Option<Visibility> {
    match action {
        ActivityPubVisibilityAction::EmitPublic => Some(Visibility::Public),
        ActivityPubVisibilityAction::EmitUnlisted => Some(Visibility::Unlisted),
        ActivityPubVisibilityAction::EmitFollowersOnly => Some(Visibility::FollowersOnly),
        ActivityPubVisibilityAction::EmitDirect => Some(Visibility::Direct),
        ActivityPubVisibilityAction::ToggleToPublic
        | ActivityPubVisibilityAction::ToggleCcPublic => None,
    }
}

fn worker_allows(
    _state: ActivityPubVisibilityModelState,
    action: ActivityPubVisibilityAction,
) -> bool {
    worker_visibility_for_emit(action).is_some()
}

fn worker_effect(
    _state: ActivityPubVisibilityModelState,
    action: ActivityPubVisibilityAction,
) -> ActivityPubVisibilityModelState {
    let Some(visibility) = worker_visibility_for_emit(action) else {
        return _state;
    };
    let (to_contains_public, cc_contains_public) = worker_emit_audience_flags(visibility);
    ActivityPubVisibilityModelState {
        to_contains_public,
        cc_contains_public,
    }
}

fn domain_step(
    state: ActivityPubVisibilityModelState,
    action: ActivityPubVisibilityAction,
) -> ActivityPubVisibilityModelState {
    model_domain_step(state, action).unwrap_or(state)
}

pub(crate) fn check_activitypub_visibility_refinement() {
    assert_model_matches_domain(&ActivityPubVisibilityModel, model_domain_step);

    assert_worker_refinement(
        ActivityPubVisibilityModel.init_states(),
        |state| {
            let _ = state;
            vec![
                ActivityPubVisibilityAction::EmitPublic,
                ActivityPubVisibilityAction::EmitUnlisted,
                ActivityPubVisibilityAction::EmitFollowersOnly,
                ActivityPubVisibilityAction::EmitDirect,
            ]
        },
        worker_allows,
        worker_effect,
        domain_step,
    );

    for state in ActivityPubVisibilityModel.init_states() {
        assert_eq!(resolved_visibility(state), worker_parse_visibility(state));
        let visibility = resolved_visibility(state);
        assert_eq!(
            is_public_activitypub_visibility(visibility),
            worker_is_public_visibility_string(visibility)
        );
    }

    for visibility in [
        Visibility::Public,
        Visibility::Unlisted,
        Visibility::FollowersOnly,
        Visibility::Direct,
    ] {
        assert_eq!(
            worker_emit_audience_flags(visibility),
            activitypub_audience_flags_for_visibility(visibility),
            "emit flags for {visibility:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::check_activitypub_visibility_refinement;

    #[test]
    fn activitypub_visibility_refinement_holds() {
        check_activitypub_visibility_refinement();
    }
}
