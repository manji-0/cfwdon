use cfwdon_domain::{
    QuoteApprovalPolicy, Visibility, activitypub_audience_flags_for_visibility,
    is_public_activitypub_visibility, visibility_from_activitypub_audiences,
};
use stateright::{Checker, Model, Property};

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActivityPubVisibilityModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ActivityPubVisibilityModelState {
    to_contains_public: bool,
    cc_contains_public: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ActivityPubVisibilityAction {
    ToggleToPublic,
    ToggleCcPublic,
    EmitPublic,
    EmitUnlisted,
    EmitFollowersOnly,
    EmitDirect,
}

impl ActivityPubVisibilityModel {
    fn resolved_visibility(state: &ActivityPubVisibilityModelState) -> Visibility {
        visibility_from_activitypub_audiences(state.to_contains_public, state.cc_contains_public)
    }
}

impl Model for ActivityPubVisibilityModel {
    type State = ActivityPubVisibilityModelState;
    type Action = ActivityPubVisibilityAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![
            ActivityPubVisibilityModelState {
                to_contains_public: false,
                cc_contains_public: false,
            },
            ActivityPubVisibilityModelState {
                to_contains_public: true,
                cc_contains_public: false,
            },
            ActivityPubVisibilityModelState {
                to_contains_public: false,
                cc_contains_public: true,
            },
            ActivityPubVisibilityModelState {
                to_contains_public: true,
                cc_contains_public: true,
            },
        ]
    }

    fn actions(&self, _state: &Self::State, actions: &mut Vec<Self::Action>) {
        actions.extend([
            ActivityPubVisibilityAction::ToggleToPublic,
            ActivityPubVisibilityAction::ToggleCcPublic,
            ActivityPubVisibilityAction::EmitPublic,
            ActivityPubVisibilityAction::EmitUnlisted,
            ActivityPubVisibilityAction::EmitFollowersOnly,
            ActivityPubVisibilityAction::EmitDirect,
        ]);
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;

        match action {
            ActivityPubVisibilityAction::ToggleToPublic => {
                next.to_contains_public = !next.to_contains_public;
            }
            ActivityPubVisibilityAction::ToggleCcPublic => {
                next.cc_contains_public = !next.cc_contains_public;
            }
            ActivityPubVisibilityAction::EmitPublic => {
                let (to, cc) = activitypub_audience_flags_for_visibility(Visibility::Public);
                next.to_contains_public = to;
                next.cc_contains_public = cc;
            }
            ActivityPubVisibilityAction::EmitUnlisted => {
                let (to, cc) = activitypub_audience_flags_for_visibility(Visibility::Unlisted);
                next.to_contains_public = to;
                next.cc_contains_public = cc;
            }
            ActivityPubVisibilityAction::EmitFollowersOnly => {
                let (to, cc) = activitypub_audience_flags_for_visibility(Visibility::FollowersOnly);
                next.to_contains_public = to;
                next.cc_contains_public = cc;
            }
            ActivityPubVisibilityAction::EmitDirect => {
                let (to, cc) = activitypub_audience_flags_for_visibility(Visibility::Direct);
                next.to_contains_public = to;
                next.cc_contains_public = cc;
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "to_public_takes_precedence",
                |_, state: &ActivityPubVisibilityModelState| {
                    !state.to_contains_public
                        || Self::resolved_visibility(state) == Visibility::Public
                },
            ),
            Property::always(
                "cc_public_without_to_is_unlisted",
                |_, state: &ActivityPubVisibilityModelState| {
                    state.to_contains_public
                        || !state.cc_contains_public
                        || Self::resolved_visibility(state) == Visibility::Unlisted
                },
            ),
            Property::always(
                "no_public_audience_is_followers_only",
                |_, state: &ActivityPubVisibilityModelState| {
                    state.to_contains_public
                        || state.cc_contains_public
                        || Self::resolved_visibility(state) == Visibility::FollowersOnly
                },
            ),
            Property::always(
                "public_activitypub_visibility_matches_public_or_unlisted",
                |_, state: &ActivityPubVisibilityModelState| {
                    let visibility = Self::resolved_visibility(state);
                    is_public_activitypub_visibility(visibility)
                        == matches!(visibility, Visibility::Public | Visibility::Unlisted)
                },
            ),
            Property::always(
                "restricted_visibility_forces_nobody_quote_policy",
                |_, state: &ActivityPubVisibilityModelState| {
                    let visibility = Self::resolved_visibility(state);
                    !visibility.is_restricted()
                        || QuoteApprovalPolicy::for_status_visibility(
                            visibility,
                            Some(QuoteApprovalPolicy::Public),
                            QuoteApprovalPolicy::Followers,
                        ) == QuoteApprovalPolicy::Nobody
                },
            ),
            Property::sometimes(
                "public_visibility_reachable",
                |_, state: &ActivityPubVisibilityModelState| {
                    Self::resolved_visibility(state) == Visibility::Public
                },
            ),
            Property::sometimes(
                "unlisted_visibility_reachable",
                |_, state: &ActivityPubVisibilityModelState| {
                    Self::resolved_visibility(state) == Visibility::Unlisted
                },
            ),
            Property::sometimes(
                "followers_only_visibility_reachable",
                |_, state: &ActivityPubVisibilityModelState| {
                    Self::resolved_visibility(state) == Visibility::FollowersOnly
                },
            ),
        ]
    }
}

pub(crate) fn check_activitypub_visibility_model() {
    ActivityPubVisibilityModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_activitypub_visibility_model;

    #[test]
    fn activitypub_visibility_model_holds() {
        check_activitypub_visibility_model();
    }
}
