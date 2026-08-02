# D1 Sessions API Feasibility Spike

**Track:** S
**Status:** Typed Sessions API is available on workers-rs `0.8.5`. Dependency upgrade is done; production route wiring is still intentionally deferred.

## Scope And Repository Context

<!-- constrained-by ../reference/configuration.md#cloudflare-bindings -->
<!-- constrained-by ../architecture/cfwdon-architecture.md#runtime-boundaries -->

This spike evaluates the D1 Sessions API. It does not change the `DB` binding, route registration, storage signatures, or request behavior. Coordination facts for production are a primary database in APAC and `read_replication.mode=auto`.

## Evidence

- The workspace requests `worker = "0.8.5"` and `worker-macros = "0.8.5"` in [`Cargo.toml`](../../Cargo.toml#L23-L24). The lockfile resolves `worker`, `worker-macros`, and `worker-sys` to `0.8.5`.
- workers-rs `0.8.x` exposes typed Sessions support:
  - `D1Database::with_session(...)`
  - `D1Database::with_session_constraint(...)`
  - `D1DatabaseSession` with `prepare`, `batch`, and `get_bookmark`
  - `D1SessionConstraint::{FirstPrimary, FirstUnconstrained}`
- The typed API shipped in [workers-rs v0.8.0](https://github.com/cloudflare/workers-rs/releases/tag/v0.8.0) via [PR #943](https://github.com/cloudflare/workers-rs/pull/943).
- Cloudflare documents `withSession()` as synchronous, returning a `D1DatabaseSession` with `prepare`, `batch`, and `getBookmark`. It documents `first-primary`, `first-unconstrained`, and bookmark inputs, and states that read replication requires the Sessions API; direct D1 queries continue to use the primary. See [D1 Database](https://developers.cloudflare.com/d1/worker-api/d1-database/#withsession) and [Global read replication](https://developers.cloudflare.com/d1/best-practices/read-replication/).

## Remaining Work After The Dependency Upgrade

Typed bindings remove the previous workers-rs `0.7.x` blocker. Application integration still needs:

1. A thin adapter that opts selected request paths into `with_session` / `with_session_constraint` instead of direct `D1Database` use.
2. A decision per path for `FirstPrimary`, `FirstUnconstrained`, or propagated bookmarks.
3. Deployed D1 coverage that asserts replica routing (`served_by_region` / bookmark round-trip) before broad rollout.
4. Care that one logical request stays on a single session so sequential consistency is preserved.

Raw `JsValue` reflection is no longer the preferred path.

## N_seq Implications

`N_seq` here means request/query sequences anchored by a D1 Session and its bookmark:

- **Before the workers-rs upgrade:** `N_seq = 0`. Direct `D1Database` calls stay on the primary even when read replication is enabled.
- **After the dependency upgrade alone:** still `N_seq = 0` until routes are wired to Sessions.
- **After a future typed integration:** `N_seq` increases only for opted-in request/session flows, normally one session per logical request.

## Recommendation

1. Keep `worker` / `worker-macros` / `worker-build` aligned on `0.8.5` (or newer matching releases).
2. Prefer `D1Database::with_session` / `with_session_constraint` and typed bookmark access over raw reflection.
3. Add a compile-checked adapter and deployed D1 integration coverage before deciding which request classes should use `first-primary`, `first-unconstrained`, or propagated bookmarks.
4. Start with read-heavy Mastodon API paths (timelines, status show, notifications) once N_seq batching work is stable.
