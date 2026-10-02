# D1 Sessions API Feasibility Spike

**Track:** S
**Status:** implemented for the initial read-heavy route set (workers-rs `0.8.5`, `1e65bae`); expansion to further read paths and production observation remain open (see Remaining Work).

## Scope And Repository Context

<!-- constrained-by ../reference/configuration.md#cloudflare-bindings -->
<!-- constrained-by ../architecture/cfwdon-architecture.md#runtime-boundaries -->

Production coordination facts: D1 primary in APAC with `read_replication.mode=auto`. Direct `D1Database` calls stay on the primary; Sessions are required to use replicas.

## Implementation

Adapter: [`crates/cfwdon-worker/src/db_session.rs`](../../crates/cfwdon-worker/src/db_session.rs)

- `open_request_session` / `open_bound_request_session` create a request-scoped session.
- Anchor selection:
  - `x-d1-bookmark` when present
  - `first-unconstrained` for GET/HEAD
  - `first-primary` for mutating methods
- `D1RequestSession::as_db` / `db_handle` re-view the session as the existing `&D1Database` prepare/batch surface (session JS objects expose the same methods).
- `with_d1_bookmark` writes `x-d1-bookmark` on successful responses when a bookmark exists.

Wired routes (initial set):

- Timelines: home, public, tag, link, direct
- Notifications: list/v2/group/entry/unread + dismiss/clear (mutations use `first-primary` when no bookmark)
- Status detail: show, card, reblogged/favourited-by, source, context, history
- Instance: `instance.rs` (two handlers) also opens a bound session

## Remaining Work

1. Expand Sessions to more read paths (search, instance directories, account timelines) once this set is stable in production metrics. Open: `search/api.rs`, `accounts/statuses/routes.rs`, and `instance/trending_links.rs` still use `bind_request_d1` (direct primary binding).
2. Observe `served_by_region` / `served_by_primary` in D1 query insights after deploy.
3. Decide whether clients should persist `x-d1-bookmark` across requests (optional continuity). The Worker already accepts and emits the header; `web-ui` does not send it today.
4. Keep write-heavy federation/inbox paths on primary-anchored sessions or direct bindings until measured.

## Recommendation

Prefer typed Sessions from workers-rs over raw reflection. Keep `worker` / `worker-macros` / `worker-build` aligned. Do not call `dump` / `exec` / `with_session` on session-derived `D1Database` handles.
