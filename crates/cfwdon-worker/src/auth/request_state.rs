//! What one request learned while it resolved its credentials: an Auth0
//! session refreshed from its refresh cookie, and why a credential it
//! presented authorized no account.
//!
//! Requests interleave on one isolate, so this state cannot live in a plain
//! thread-local. [`scope_request_auth_state`] gives each request its own
//! [`RequestAuthState`] and installs it in the slot only while that request's
//! future is being polled; every other request sees its own state there.

use cfwdon_core::AuthenticatedUser;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Why a valid credential authorized no account. Handlers that only see
/// "no account" answer `401`; the router turns that into the matching `403`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CredentialRefusal {
    /// The OAuth token's scopes do not cover the endpoint.
    ScopeDenied,
    /// The account waits for an admin's approval (Mastodon's `require_user!`).
    PendingApproval,
}

#[derive(Clone, Debug)]
pub(super) struct RefreshedAuth0Session {
    pub(super) user: AuthenticatedUser,
    pub(super) access_token: String,
    pub(super) refresh_token: Option<String>,
    pub(super) access_max_age: i64,
}

#[derive(Debug, Default)]
pub(crate) struct RequestAuthState {
    refreshed_auth0_session: Option<RefreshedAuth0Session>,
    refusal: Option<CredentialRefusal>,
}

thread_local! {
    /// The state of the request whose future is being polled right now.
    static CURRENT: RefCell<RequestAuthState> = RefCell::new(RequestAuthState::default());
}

/// Run `request` with its own, initially empty, auth state.
pub(crate) fn scope_request_auth_state<F: Future>(request: F) -> RequestScoped<F> {
    RequestScoped {
        state: RequestAuthState::default(),
        request: Box::pin(request),
    }
}

pub(crate) struct RequestScoped<F> {
    state: RequestAuthState,
    request: Pin<Box<F>>,
}

impl<F: Future> Future for RequestScoped<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.get_mut();
        let outer = CURRENT.with(|slot| slot.replace(std::mem::take(&mut this.state)));
        let poll = this.request.as_mut().poll(cx);
        this.state = CURRENT.with(|slot| slot.replace(outer));
        poll
    }
}

pub(super) fn refreshed_auth0_session() -> Option<RefreshedAuth0Session> {
    CURRENT.with(|slot| slot.borrow().refreshed_auth0_session.clone())
}

pub(super) fn record_refreshed_auth0_session(session: RefreshedAuth0Session) {
    CURRENT.with(|slot| slot.borrow_mut().refreshed_auth0_session = Some(session));
}

pub(crate) fn record_credential_refusal(refusal: CredentialRefusal) {
    CURRENT.with(|slot| slot.borrow_mut().refusal = Some(refusal));
}

/// Why this request's credential authorized no account, if it did not.
pub(crate) fn credential_refusal() -> Option<CredentialRefusal> {
    CURRENT.with(|slot| slot.borrow().refusal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use std::task::Waker;

    /// Yields once, so two scoped requests can interleave.
    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                return Poll::Ready(());
            }
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    #[test]
    fn interleaved_requests_keep_their_own_refusal() {
        let mut pending = Box::pin(scope_request_auth_state(async {
            record_credential_refusal(CredentialRefusal::PendingApproval);
            YieldOnce(false).await;
            credential_refusal()
        }));
        let mut clean = Box::pin(scope_request_auth_state(async {
            YieldOnce(false).await;
            credential_refusal()
        }));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(pending.as_mut().poll(&mut cx).is_pending());
        assert!(clean.as_mut().poll(&mut cx).is_pending());
        assert_eq!(credential_refusal(), None, "no state leaks outside a scope");
        assert_eq!(clean.as_mut().poll(&mut cx), Poll::Ready(None));
        assert_eq!(
            pending.as_mut().poll(&mut cx),
            Poll::Ready(Some(CredentialRefusal::PendingApproval))
        );
    }

    #[test]
    fn a_new_request_starts_without_a_refusal() {
        record_credential_refusal(CredentialRefusal::ScopeDenied);
        let inner = scope_request_auth_state(async { credential_refusal() })
            .now_or_never()
            .unwrap();
        assert_eq!(inner, None);
        assert_eq!(
            credential_refusal(),
            Some(CredentialRefusal::ScopeDenied),
            "the outer state comes back after the scoped poll"
        );
    }
}
