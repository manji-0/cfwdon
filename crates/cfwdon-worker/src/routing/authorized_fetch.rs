//! The secure-mode (`AUTHORIZED_FETCH`) gate in front of the guarded
//! ActivityPub routes. `cfwdon_domain::authorized_fetch_access` decides; the
//! gate verifies the signature, answers a refusal, and marks every secure-mode
//! answer `private` with `Vary: Signature`.
//!
//! Routes without the gate stay public in secure mode: the instance actor
//! (`/actor`), WebFinger, host-meta and NodeInfo. A remote server fetches
//! `/actor` to verify the signed GETs secure mode asks of it, so it must answer
//! unsigned.

use crate::authorized_fetch::{
    FetchRequester, FetchViewer, activitypub_request_prefers_html, mark_signature_dependent,
};
use crate::db_session::bind_request_d1;
use crate::http::{SignedFetch, verify_signed_fetch};
use crate::observability::log_json_event;
use crate::response_utils::json_response;
use crate::runtime_config::load_config;
use cfwdon_domain::{FetchedResource, authorized_fetch_access};
use std::future::Future;
use worker::{Request, Response, Result, RouteContext};

/// A guarded route, by what it serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuardedRoute {
    /// `/users/:username` and `/@:username`: an actor, or its HTML profile.
    Actor,
    /// `/users/:username/statuses/:id` and `/@:username/:id`: a note, or its
    /// HTML page.
    Note,
    /// The outbox, the featured posts and tags collections, and quote
    /// stamps: the owner's content, served only as ActivityPub.
    Content,
    /// The follower and following collections, served only as ActivityPub.
    SocialGraph,
}

impl GuardedRoute {
    fn resource(self) -> FetchedResource {
        match self {
            Self::Actor => FetchedResource::Actor,
            Self::Note | Self::Content => FetchedResource::OwnerContent,
            Self::SocialGraph => FetchedResource::SocialGraph,
        }
    }

    /// The HTML pages stay public, as Mastodon's web pages do in secure mode.
    fn serves_html_to(self, req: &Request) -> Result<bool> {
        match self {
            Self::Actor | Self::Note => activitypub_request_prefers_html(req),
            Self::Content | Self::SocialGraph => Ok(false),
        }
    }
}

/// Run `handler` for a GET of a guarded route, as secure mode allows.
pub(crate) async fn authorized_activitypub_fetch<F, Fut>(
    req: Request,
    ctx: RouteContext<()>,
    route: GuardedRoute,
    handler: F,
) -> Result<Response>
where
    F: FnOnce(Request, RouteContext<()>, FetchViewer) -> Fut,
    Fut: Future<Output = Result<Response>>,
{
    let config = load_config(&ctx);
    if !config.authorized_fetch || route.serves_html_to(&req)? {
        let viewer = FetchViewer::new(FetchRequester::Anyone, route.resource());
        return handler(req, ctx, viewer).await;
    }
    let db = bind_request_d1(&ctx, &config)?;
    let signed = verify_signed_fetch(&req, &db).await?;
    let access = authorized_fetch_access(true, route.resource(), signed.verdict());
    if let Some(status) = access.denial_status() {
        log_refused_fetch(&req, &signed, status);
        return mark_signature_dependent(
            json_response(
                &serde_json::json!({ "error": refusal_message(&signed) }),
                "application/json; charset=utf-8",
                &[],
            )?
            .with_status(status),
        );
    }
    let requester = match signed {
        SignedFetch::Signed(actor) => FetchRequester::Signed(actor),
        SignedFetch::Unsigned | SignedFetch::Invalid(_) | SignedFetch::Refused => {
            FetchRequester::Restricted
        }
    };
    let viewer = FetchViewer::new(requester, route.resource());
    mark_signature_dependent(handler(req, ctx, viewer).await?)
}

/// Mastodon's `signature_verification_failure_reason` wording.
fn refusal_message(signed: &SignedFetch) -> &'static str {
    match signed {
        SignedFetch::Unsigned => "Request not signed",
        SignedFetch::Invalid(_) => "Verification failed for the request's signature",
        SignedFetch::Refused => "This instance does not federate with the signer's domain",
        SignedFetch::Signed(_) => "Forbidden",
    }
}

fn log_refused_fetch(req: &Request, signed: &SignedFetch, status: u16) {
    log_json_event(serde_json::json!({
        "event": "authorized_fetch_refused",
        "path": req.path(),
        "status": status,
        "verdict": format!("{:?}", signed.verdict()),
        "error": match signed {
            SignedFetch::Invalid(error) => Some(error.to_string()),
            _ => None,
        },
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::federation::RemoteActorProfile;

    #[test]
    fn owner_blocks_reach_only_routes_that_serve_the_owners_content() {
        for (route, consults_owner_blocks) in [
            (GuardedRoute::Actor, false),
            (GuardedRoute::Note, true),
            (GuardedRoute::Content, true),
            (GuardedRoute::SocialGraph, false),
        ] {
            let signed = FetchViewer::new(
                FetchRequester::Signed(RemoteActorProfile::default()),
                route.resource(),
            );
            assert_eq!(
                signed.signer_subject_to_owner_blocks().is_some(),
                consults_owner_blocks,
                "{route:?}"
            );
            let unsigned = FetchViewer::new(FetchRequester::Restricted, route.resource());
            assert!(
                unsigned.signer_subject_to_owner_blocks().is_none(),
                "{route:?}"
            );
        }
    }
}
