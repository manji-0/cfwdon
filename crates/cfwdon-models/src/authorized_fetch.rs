use cfwdon_domain::{
    FetchAccess, FetchedBody, FetchedResource, SignedFetchVerdict, authorized_fetch_access,
    authorized_fetch_body,
};
use stateright::{Checker, Model, Property};

/// Secure mode (`AUTHORIZED_FETCH`) for one GET of a local ActivityPub
/// resource: the switch, what is asked for, what the signature says, and
/// whether the resource's owner blocks the signer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AuthorizedFetchModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AuthorizedFetchModelState {
    pub(crate) secure_mode: bool,
    pub(crate) resource: FetchedResource,
    pub(crate) verdict: SignedFetchVerdict,
    pub(crate) signer_blocked_by_owner: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AuthorizedFetchAction {
    ToggleSecureMode,
    CycleResource,
    CycleVerdict,
    ToggleOwnerBlock,
}

pub(crate) const VERDICTS: [SignedFetchVerdict; 4] = [
    SignedFetchVerdict::Unsigned,
    SignedFetchVerdict::Invalid,
    SignedFetchVerdict::Refused,
    SignedFetchVerdict::Signed,
];

pub(crate) const RESOURCES: [FetchedResource; 2] =
    [FetchedResource::Actor, FetchedResource::Object];

impl AuthorizedFetchModel {
    pub(crate) fn access(state: &AuthorizedFetchModelState) -> FetchAccess {
        authorized_fetch_access(state.secure_mode, state.resource, state.verdict)
    }

    pub(crate) fn body(state: &AuthorizedFetchModelState) -> FetchedBody {
        authorized_fetch_body(Self::access(state), state.signer_blocked_by_owner)
    }

    fn cycle_verdict(verdict: SignedFetchVerdict) -> SignedFetchVerdict {
        match verdict {
            SignedFetchVerdict::Unsigned => SignedFetchVerdict::Invalid,
            SignedFetchVerdict::Invalid => SignedFetchVerdict::Refused,
            SignedFetchVerdict::Refused => SignedFetchVerdict::Signed,
            SignedFetchVerdict::Signed => SignedFetchVerdict::Unsigned,
        }
    }

    fn cycle_resource(resource: FetchedResource) -> FetchedResource {
        match resource {
            FetchedResource::Actor => FetchedResource::Object,
            FetchedResource::Object => FetchedResource::Actor,
        }
    }
}

impl Model for AuthorizedFetchModel {
    type State = AuthorizedFetchModelState;
    type Action = AuthorizedFetchAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![AuthorizedFetchModelState {
            secure_mode: false,
            resource: FetchedResource::Actor,
            verdict: SignedFetchVerdict::Unsigned,
            signer_blocked_by_owner: false,
        }]
    }

    fn actions(&self, _state: &Self::State, actions: &mut Vec<Self::Action>) {
        actions.extend([
            AuthorizedFetchAction::ToggleSecureMode,
            AuthorizedFetchAction::CycleResource,
            AuthorizedFetchAction::CycleVerdict,
            AuthorizedFetchAction::ToggleOwnerBlock,
        ]);
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = *state;
        match action {
            AuthorizedFetchAction::ToggleSecureMode => next.secure_mode = !next.secure_mode,
            AuthorizedFetchAction::CycleResource => {
                next.resource = Self::cycle_resource(next.resource);
            }
            AuthorizedFetchAction::CycleVerdict => {
                next.verdict = Self::cycle_verdict(next.verdict);
            }
            AuthorizedFetchAction::ToggleOwnerBlock => {
                next.signer_blocked_by_owner = !next.signer_blocked_by_owner;
            }
        }
        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "unsigned_request_never_receives_a_non_public_resource_in_secure_mode",
                |_, state: &AuthorizedFetchModelState| {
                    !state.secure_mode
                        || state.verdict == SignedFetchVerdict::Signed
                        || matches!(
                            AuthorizedFetchModel::body(state),
                            FetchedBody::RestrictedActor | FetchedBody::Denied
                        )
                },
            ),
            Property::always(
                "objects_without_a_valid_signature_are_unauthorized",
                |_, state: &AuthorizedFetchModelState| {
                    !state.secure_mode
                        || state.resource != FetchedResource::Object
                        || !matches!(
                            state.verdict,
                            SignedFetchVerdict::Unsigned | SignedFetchVerdict::Invalid
                        )
                        || AuthorizedFetchModel::access(state).denial_status() == Some(401)
                },
            ),
            Property::always(
                "unsigned_actor_keeps_its_key_readable",
                |_, state: &AuthorizedFetchModelState| {
                    state.resource != FetchedResource::Actor
                        || state.verdict == SignedFetchVerdict::Refused
                        || AuthorizedFetchModel::access(state)
                            .denial_status()
                            .is_none()
                },
            ),
            Property::always(
                "refused_signer_is_forbidden_in_secure_mode",
                |_, state: &AuthorizedFetchModelState| {
                    !state.secure_mode
                        || state.verdict != SignedFetchVerdict::Refused
                        || AuthorizedFetchModel::access(state).denial_status() == Some(403)
                },
            ),
            Property::always(
                "blocked_signer_never_sees_the_owners_resource",
                |_, state: &AuthorizedFetchModelState| {
                    !state.secure_mode
                        || !state.signer_blocked_by_owner
                        || AuthorizedFetchModel::body(state) != FetchedBody::Whole
                },
            ),
            Property::always(
                "restricted_document_only_for_actors",
                |_, state: &AuthorizedFetchModelState| {
                    AuthorizedFetchModel::body(state) != FetchedBody::RestrictedActor
                        || state.resource == FetchedResource::Actor
                },
            ),
            Property::always(
                "secure_mode_answers_are_never_shared_cacheable",
                |_, state: &AuthorizedFetchModelState| {
                    !state.secure_mode || !AuthorizedFetchModel::access(state).shared_cacheable()
                },
            ),
            Property::always(
                "secure_mode_off_answers_everyone",
                |_, state: &AuthorizedFetchModelState| {
                    state.secure_mode || AuthorizedFetchModel::body(state) == FetchedBody::Whole
                },
            ),
            Property::sometimes(
                "signed_fetch_gets_the_whole_resource",
                |_, state: &AuthorizedFetchModelState| {
                    state.secure_mode && AuthorizedFetchModel::body(state) == FetchedBody::Whole
                },
            ),
            Property::sometimes(
                "unsigned_actor_gets_the_restricted_document",
                |_, state: &AuthorizedFetchModelState| {
                    AuthorizedFetchModel::body(state) == FetchedBody::RestrictedActor
                },
            ),
            Property::sometimes(
                "blocked_signer_gets_nothing_of_the_owner",
                |_, state: &AuthorizedFetchModelState| {
                    AuthorizedFetchModel::body(state) == FetchedBody::Hidden
                },
            ),
            Property::sometimes(
                "unsigned_object_fetch_is_refused",
                |_, state: &AuthorizedFetchModelState| {
                    AuthorizedFetchModel::body(state) == FetchedBody::Denied
                },
            ),
        ]
    }
}

pub(crate) fn check_authorized_fetch_model() {
    AuthorizedFetchModel
        .checker()
        .spawn_dfs()
        .join()
        .assert_properties();
}

#[cfg(test)]
mod tests {
    use super::check_authorized_fetch_model;

    #[test]
    fn authorized_fetch_model_holds() {
        check_authorized_fetch_model();
    }
}
