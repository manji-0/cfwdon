# D1 Sessions API Feasibility Spike

**Track:** S
**Status:** Feasible at the JavaScript runtime boundary, blocked in the locked Rust binding, and intentionally not wired into production routes.

## Scope And Repository Context

<!-- constrained-by ../reference/configuration.md#cloudflare-bindings -->
<!-- constrained-by ../architecture/cfwdon-architecture.md#runtime-boundaries -->

This spike evaluates the D1 Sessions API only. It does not change the `DB` binding, route registration, storage signatures, or request behavior. Coordination facts for production are a primary database in APAC and `read_replication.mode=auto`.

## Evidence

- The workspace requests `worker = "0.7.4"` and `worker-macros = "0.7.4"` in [`Cargo.toml`](../../Cargo.toml#L23-L24). The semver range resolves to `worker`, `worker-macros`, and `worker-sys` `0.7.5` in [`Cargo.lock`](../../Cargo.lock#L1953-L2008).
- The local `worker` 0.7.5 D1 wrapper exposes `prepare`, `dump`, `batch`, and `exec`, but no `with_session` or session wrapper (`worker-0.7.5/src/d1/mod.rs`).
- The local `worker-sys` 0.7.5 D1 externs expose the same four database methods and no `withSession`, `D1DatabaseSession`, or `getBookmark` (`worker-sys-0.7.5/src/types/d1.rs`).
- The worker crate already depends on `js-sys`, `wasm-bindgen`, and `wasm-bindgen-futures`, and existing code uses `Reflect`, `JsValue`, and `JsFuture` for Web APIs.
- Cloudflare documents `withSession()` as synchronous, returning a `D1DatabaseSession` with `prepare`, `batch`, and `getBookmark`. It documents `first-primary`, `first-unconstrained`, and bookmark inputs, and states that read replication requires the Sessions API; direct D1 queries continue to use the primary. See [D1 Database](https://developers.cloudflare.com/d1/worker-api/d1-database/#withsession) and [Global read replication](https://developers.cloudflare.com/d1/best-practices/read-replication/).
- The missing binding was added upstream by [workers-rs PR #943](https://github.com/cloudflare/workers-rs/pull/943), titled `feat(d1): add typed session API and tests`, and shipped in [workers-rs v0.8.0](https://github.com/cloudflare/workers-rs/releases/tag/v0.8.0).

## Interop Feasibility

Raw interop is technically feasible without a new dependency or a direct `worker-sys` dependency:

1. Read the binding object through `D1Database`'s public `AsRef<JsValue>` implementation.
2. Reflect `withSession` and call it with the database as `this` and an optional constraint or bookmark string.
3. Keep the returned session as an opaque `JsValue`.
4. Reflect session `prepare`, `batch`, and `getBookmark`, then reflect prepared-statement `bind`, `first`, `run`, `all`, or `raw`.
5. Convert promise results with `JsFuture`, deserialize rows with the existing `serde_wasm_bindgen` path, and map both synchronous JS exceptions and rejected D1 promises to `worker::Error`.

The wrapper would need to preserve the JavaScript receiver for every method call. `withSession` and `getBookmark` are synchronous; statement execution and session `batch` return promises. The wrapper would also need explicit handling for `null` bookmarks and optional `withSession()` arguments.

## Exact Blocker

The current `worker`/`worker-sys` 0.7.x API has no typed entry point or low-level binding for Sessions. A raw wrapper can call the runtime, but it cannot reuse the existing `worker::D1PreparedStatement` abstraction: the session statement is returned as a different opaque JavaScript object, and the existing wrapper's inner binding is private. Existing stores and `worker::query!` are written around `&D1Database` and `worker::D1PreparedStatement`.

Therefore the blocker is the upstream workers-rs binding version, not a Cloudflare runtime limitation and not a missing local WebAssembly capability. A compile-checked standalone stub would also require declaring a new module in the existing `src/lib.rs`, which is outside this track's exclusive new-file ownership. No stub is added here.

## Raw Interop Risks

- Method names, receiver binding, optional arguments, return shapes, and the `D1DatabaseSession` object are not checked by Rust at compile time.
- A runtime or emulator without `withSession` would fail only when that path executes; local and deployed behavior must both be tested.
- JS exceptions, promise rejections, D1 errors, and `null` bookmarks need one consistent conversion policy.
- A raw session statement cannot flow through current D1 helper signatures without a broad adapter or storage refactor, increasing duplication and the chance of mixing session and non-session queries in one logical request.
- Bookmark propagation is an application protocol concern. A session created per request gives consistency within that request; cross-request continuity requires carrying the latest bookmark into the next session and returning the new bookmark.
- The existing APAC-primary deployment would gain no read-replica latency from a wrapper that accidentally falls back to direct `D1Database` calls or fails to use the same session for every query.

## N_seq Implications

`N_seq` is not defined elsewhere in this repository. This note uses it as the number of request/query sequences explicitly anchored by a D1 Session and its bookmark:

- **Before this spike:** `N_seq = 0`. Existing code calls the direct D1 binding. With read replication enabled, Cloudflare says those calls remain on the primary, so the application has no replica-eligible Session sequence or bookmark continuity. This does not mean primary writes and reads are unordered; it means the Sessions API guarantee is not being used.
- **After this spike:** `N_seq = 0`. This is a docs-only feasibility result; no route or storage path changes.
- **After a future typed integration:** `N_seq` increases only for opted-in request/session flows, normally one session per logical request. Queries using one session receive sequential consistency; cross-request sequencing requires bookmark input/output. Existing non-opted-in paths remain outside `N_seq`.

## Recommendation

1. Upgrade `worker` and `worker-macros` together to a workers-rs release that includes the typed Sessions API, currently `0.8.0` or later, in a separate dependency-owned change. The upstream implementation already exists; an additional upstream PR is not needed.
2. Prefer `D1Database::with_session`, `D1DatabaseSession`, typed constraints, and bookmark access from workers-rs over raw reflection.
3. After the dependency upgrade, add a compile-checked adapter and deployed D1 integration coverage before deciding which request classes should use `first-primary`, `first-unconstrained`, or propagated bookmarks.
4. If remaining on 0.7.x is mandatory, request or maintain a workers-rs backport/fork with the typed bindings. Use raw reflection only as an isolated, temporary adapter with explicit tests, not as route-wide production plumbing.
