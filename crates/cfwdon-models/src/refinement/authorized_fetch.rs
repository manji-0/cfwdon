use cfwdon_domain::{
    FetchAccess, FetchedBody, FetchedResource, SignedFetchVerdict, authorized_fetch_access,
};
use stateright::Model;

use crate::authorized_fetch::{
    AuthorizedFetchAction, AuthorizedFetchModel, AuthorizedFetchModelState, RESOURCES, VERDICTS,
};
use crate::refinement::verify::assert_model_matches_domain;

/// Mirrors the worker's `routing::authorized_fetch::GuardedRoute`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkerRoute {
    Actor,
    Note,
    Content,
    SocialGraph,
}

const ROUTES: [WorkerRoute; 4] = [
    WorkerRoute::Actor,
    WorkerRoute::Note,
    WorkerRoute::Content,
    WorkerRoute::SocialGraph,
];

/// Mirrors `GuardedRoute::resource`.
fn worker_route_resource(route: WorkerRoute) -> FetchedResource {
    match route {
        WorkerRoute::Actor => FetchedResource::Actor,
        WorkerRoute::Note | WorkerRoute::Content => FetchedResource::OwnerContent,
        WorkerRoute::SocialGraph => FetchedResource::SocialGraph,
    }
}

/// What each route's handlers do for a signer the owner blocks, read off the
/// handlers rather than the domain: `status_object_response` and
/// `status_quote_authorization_object_response` answer `404`;
/// `outbox_response`, `featured_collection_response` and
/// `featured_tags_collection_response` answer an empty collection;
/// `actor_response`, `followers_collection_response` and
/// `following_collection_response` never consult the owner's blocks.
fn worker_handler_hides_from_blocked_signer(route: WorkerRoute) -> bool {
    match route {
        WorkerRoute::Note | WorkerRoute::Content => true,
        WorkerRoute::Actor | WorkerRoute::SocialGraph => false,
    }
}

/// Mirrors `GuardedRoute::serves_html_to`: only actors and notes have pages.
fn worker_serves_html(route: WorkerRoute, prefers_html: bool) -> bool {
    match route {
        WorkerRoute::Actor | WorkerRoute::Note => prefers_html,
        WorkerRoute::Content | WorkerRoute::SocialGraph => false,
    }
}

/// Mirrors `authorized_activitypub_fetch`: the denial it answers, or the
/// access of the `FetchViewer` it runs the handler with.
fn worker_access(
    secure_mode: bool,
    route: WorkerRoute,
    prefers_html: bool,
    verdict: SignedFetchVerdict,
) -> FetchAccess {
    if !secure_mode || worker_serves_html(route, prefers_html) {
        return FetchAccess::Public;
    }
    let access = authorized_fetch_access(true, worker_route_resource(route), verdict);
    if access.denial_status().is_some() {
        return access;
    }
    match verdict {
        SignedFetchVerdict::Signed => FetchAccess::Signed,
        SignedFetchVerdict::Unsigned
        | SignedFetchVerdict::Invalid
        | SignedFetchVerdict::Refused => FetchAccess::Restricted,
    }
}

/// Whether the handler hides the owner's content from this request: only a
/// signed GET names a signer, and only some handlers consult the block.
fn worker_hides_owner_content(
    route: WorkerRoute,
    access: FetchAccess,
    signer_blocked_by_owner: bool,
) -> bool {
    access == FetchAccess::Signed
        && signer_blocked_by_owner
        && worker_handler_hides_from_blocked_signer(route)
}

fn model_domain_step(
    state: AuthorizedFetchModelState,
    action: AuthorizedFetchAction,
) -> Option<AuthorizedFetchModelState> {
    AuthorizedFetchModel.next_state(&state, action)
}

pub(crate) fn check_authorized_fetch_refinement() {
    assert_model_matches_domain(&AuthorizedFetchModel, model_domain_step);

    for secure_mode in [false, true] {
        for route in ROUTES {
            for verdict in VERDICTS {
                for signer_blocked_by_owner in [false, true] {
                    let state = AuthorizedFetchModelState {
                        secure_mode,
                        resource: worker_route_resource(route),
                        verdict,
                        signer_blocked_by_owner,
                    };
                    let access = worker_access(secure_mode, route, false, verdict);
                    assert_eq!(
                        access,
                        AuthorizedFetchModel::access(&state),
                        "worker gate for {route:?} in {state:?}"
                    );
                    assert_eq!(
                        worker_hides_owner_content(route, access, signer_blocked_by_owner),
                        AuthorizedFetchModel::body(&state) == FetchedBody::Hidden,
                        "owner block for {route:?} in {state:?}"
                    );

                    // A browser's Accept header opens only the HTML pages;
                    // a collection asked for as HTML is still guarded.
                    let html_access = worker_access(secure_mode, route, true, verdict);
                    if matches!(route, WorkerRoute::Content | WorkerRoute::SocialGraph) {
                        assert_eq!(html_access, access, "{route:?} asked for as HTML");
                    } else {
                        assert_eq!(html_access, FetchAccess::Public);
                    }
                }
            }
        }
    }

    for resource in RESOURCES {
        assert!(
            ROUTES
                .iter()
                .any(|route| worker_route_resource(*route) == resource),
            "every resource has a guarded route: {resource:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::check_authorized_fetch_refinement;

    #[test]
    fn authorized_fetch_refinement_holds() {
        check_authorized_fetch_refinement();
    }
}
