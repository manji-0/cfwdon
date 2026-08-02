//! D1 Sessions API helpers for request-scoped replica-aware queries.
//!
//! workers-rs exposes [`worker::D1DatabaseSession`] separately from [`worker::D1Database`].
//! Existing cfwdon storage helpers take `&D1Database` and only call `prepare` / `batch`.
//! Session JS objects implement those same methods, so this module re-views a session as the
//! `D1Database` prepare/batch surface used throughout the worker. Do not call `dump`, `exec`,
//! or `with_session` on handles returned by [`D1RequestSession::db_handle`].

use cfwdon_core::AppConfig;
use wasm_bindgen::JsCast;
use worker::{D1Database, D1DatabaseSession, Method, Request, Response, Result, RouteContext};

/// Bookmark header used by Cloudflare's D1 Sessions examples and client continuations.
pub(crate) const D1_BOOKMARK_HEADER: &str = "x-d1-bookmark";

/// Request-scoped D1 session with a prepare/batch-compatible database view.
pub(crate) struct D1RequestSession {
    session: D1DatabaseSession,
}

impl D1RequestSession {
    /// Owned prepare/batch handle that shares the same JS session object.
    pub(crate) fn db_handle(&self) -> D1Database {
        D1Database::unchecked_from_js(self.session.as_ref().clone())
    }

    /// Latest bookmark observed by this session, if any query has run.
    pub(crate) fn bookmark(&self) -> Result<Option<String>> {
        self.session.get_bookmark()
    }
}

/// Open a session for this HTTP request.
///
/// Anchor selection:
/// - `x-d1-bookmark` when present (client continuation)
/// - `first-unconstrained` for safe methods (GET/HEAD) so reads can use replicas
/// - `first-primary` for mutating methods so the first query sees the latest write state
pub(crate) fn open_request_session(db: &D1Database, req: &Request) -> Result<D1RequestSession> {
    let anchor = session_anchor_for_request(req)?;
    let session = db.with_session(anchor.as_deref())?;
    Ok(D1RequestSession { session })
}

/// Bind `DB`, open a request session, and return both the session and a db handle.
pub(crate) fn open_bound_request_session(
    ctx: &RouteContext<()>,
    config: &AppConfig,
    req: &Request,
) -> Result<(D1RequestSession, D1Database)> {
    let binding = ctx.d1(&config.database_binding)?;
    let session = open_request_session(&binding, req)?;
    let db = session.db_handle();
    Ok((session, db))
}

/// Attach the session bookmark to the response when one is available.
pub(crate) fn with_d1_bookmark(
    mut response: Response,
    session: &D1RequestSession,
) -> Result<Response> {
    if let Some(bookmark) = session.bookmark()? {
        response.headers_mut().set(D1_BOOKMARK_HEADER, &bookmark)?;
    }
    Ok(response)
}

fn session_anchor_for_request(req: &Request) -> Result<Option<String>> {
    if let Some(bookmark) = req.headers().get(D1_BOOKMARK_HEADER)? {
        let trimmed = bookmark.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_owned()));
        }
    }
    Ok(Some(default_session_constraint(req.method()).to_owned()))
}

fn default_session_constraint(method: Method) -> &'static str {
    match method {
        Method::Get | Method::Head => "first-unconstrained",
        _ => "first-primary",
    }
}

#[cfg(test)]
mod tests {
    use super::{D1_BOOKMARK_HEADER, default_session_constraint};
    use worker::Method;

    #[test]
    fn bookmark_header_constant_matches_cloudflare_docs() {
        assert_eq!(D1_BOOKMARK_HEADER, "x-d1-bookmark");
    }

    #[test]
    fn safe_methods_default_to_unconstrained_replicas() {
        assert_eq!(
            default_session_constraint(Method::Get),
            "first-unconstrained"
        );
        assert_eq!(
            default_session_constraint(Method::Head),
            "first-unconstrained"
        );
    }

    #[test]
    fn mutating_methods_default_to_primary() {
        assert_eq!(default_session_constraint(Method::Post), "first-primary");
        assert_eq!(default_session_constraint(Method::Put), "first-primary");
        assert_eq!(default_session_constraint(Method::Delete), "first-primary");
        assert_eq!(default_session_constraint(Method::Patch), "first-primary");
    }
}
