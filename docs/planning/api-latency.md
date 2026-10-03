# API Latency Reduction

**Status:** in progress on branch `perf/api-latency`.

Goal: cut the serial data round trips (D1, KV, Cache API, Durable Object, outbound HTTP) on the critical path of the main Mastodon API scenarios, without changing response shapes. Every item ends with `devbox run ci` green and one conventional commit.

## Context

<!-- constrained-by ../architecture/cfwdon-architecture.md#runtime-boundaries -->
<!-- derived-from ./d1-sessions-api-spike.md -->

The Worker is placed next to the D1 primary (`[placement] region`), so one D1 round trip is short and latency scales with the number of *serial* round trips. Read replicas (Sessions API) add little latency benefit under this placement; they are out of scope until the round-trip counts drop. Round-trip counts below are code-reading estimates, not measurements.

## Ordered Work Items

1. [x] **Deferred work via `ctx.wait_until`.** `deferred.rs` queues owned futures; `fetch` drains them under `Context::wait_until` (scheduled and queue handlers drain inline). Deferred: the outbox queue kick (`router.rs`), Web Push sends, StreamHub fan-out for status create/edit/delete and notifications, Cache API puts, public-endpoint KV puts. Kept awaited on purpose: Cache API deletes (clients re-read right after a profile write; the three deletes now run concurrently) and the account-capabilities KV put (a late put could outlive an invalidation). Local run: POST /statuses 48 → 25 D1 queries before the response, favourite 35 → 19, reblog 46 → 30.
2. [ ] **Custom emoji cache.** Cache the `custom_emojis` table behind the instance-settings L1 + KV pattern; invalidate on admin writes.
3. [ ] **List timeline.** Select candidates with SQL over `account_list_memberships` and render through the shared timeline candidate/preload path instead of filtering the public timeline row by row.
4. [ ] **Notifications.** Batch the follow / follow-request / admin collectors (accounts, actors, mutes); stop running `DELETE FROM mutes` on reads (filter by `expires_at` instead).
5. [ ] **Status context.** Load thread ids in bulk and render through the batched preload path; keep remote hydration off the request path.
6. [ ] **Conversations.** Batch participants/accounts and render last statuses through the preload path.
7. [ ] **Account statuses and status detail.** Render through the preload path; replace the serial render loop.
8. [ ] **POST /statuses.** Drop the post-insert re-fetch and duplicate response/Note builds; fold hashtag/mention/poll inserts into the existing batch.
9. [ ] **Favourite / reblog / follow.** Build the response once; batch Announce inbox inserts.
10. [ ] **Remote account reads.** Serve stored remote actors and refresh stale ones asynchronously (accounts/:id, lookup, exact-handle search).
11. [ ] **Inbox Create.** Upsert a remote status once per activity, not once per local recipient.
12. [ ] **Small items.** Single-trip invalid-token auth lookup, markers single query, parallel independent pre-handler reads.

## Done Criteria

- Every item above is checked or explicitly marked deferred with a reason.
- `devbox run ci` passes on the branch head; new or changed hot SQL is covered by `scripts/check_query_plans.py`.
- `python3 scripts/generate_mastodon_api_compat.py` is re-run if any route behavior changes.
