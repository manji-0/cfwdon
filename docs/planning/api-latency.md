# API Latency Reduction

**Status:** in progress on branch `perf/api-latency`.

Goal: cut the serial data round trips (D1, KV, Cache API, Durable Object, outbound HTTP) on the critical path of the main Mastodon API scenarios, without changing response shapes. Every item ends with `devbox run ci` green and one conventional commit.

## Context

<!-- constrained-by ../architecture/cfwdon-architecture.md#runtime-boundaries -->
<!-- derived-from ./d1-sessions-api-spike.md -->

The Worker is placed next to the D1 primary (`[placement] region`), so one D1 round trip is short and latency scales with the number of *serial* round trips. Read replicas (Sessions API) add little latency benefit under this placement; they are out of scope until the round-trip counts drop. Round-trip counts below are code-reading estimates, not measurements.

## Ordered Work Items

1. [x] **Deferred work via `ctx.wait_until`.** `deferred.rs` queues owned futures; `fetch` drains them under `Context::wait_until` (scheduled and queue handlers drain inline). Deferred: the outbox queue kick (`router.rs`), Web Push sends, StreamHub fan-out for status create/edit/delete and notifications, Cache API puts, public-endpoint KV puts. Kept awaited on purpose: Cache API deletes (clients re-read right after a profile write; the three deletes now run concurrently) and the account-capabilities KV put (a late put could outlive an invalidation). Local run: POST /statuses 48 → 25 D1 queries before the response, favourite 35 → 19, reblog 46 → 30.
2. [x] **Custom emoji cache.** `custom_emojis/store.rs` keeps the table rows in an isolate L1 with a 60 s TTL, cleared on admin create/update/delete in the same isolate. KV was skipped: with the Worker next to D1 a KV read saves little, and a second cache layer widens the stale window after admin edits.
3. [x] **List timeline.** `timelines/list_timeline.rs` resolves members (account id, `user@domain`, actor URI) in SQL and seeks each member's public statuses, then renders through the home timeline's shared `timeline_entries_from_candidate_rows`. Paging now reaches statuses older than the public-timeline window, and filters and muted authors apply as on home. Local run: 145 → 33 D1 queries.
4. [x] **Notifications.** Follow and follow-request collectors load their local/remote rows concurrently and hydrate accounts, actors, and notification mutes in one batched round (was three serial queries per row); dismissals and the clear marker load alongside the collectors. Mute reads no longer `DELETE` expired rows (they filter on `expires_at`), and the hourly cron calls `purge_expired_mutes`. Admin report/sign-up collectors stay per-row (admin-only, low volume). Local run with 30 follow notifications: 50 D1 queries, independent of the follower count.
5. [x] **Status context.** Local and remote context traversal now collects nodes and renders them once through `timelines::render_status_items` (the timeline preload path) instead of a full status build per node. For signed-in viewers the remote reply-tree fetch (`hydrate_remote_descendants_for_context`, outbound HTTP + upserts) runs after the response; the context answers with a `running` `Mastodon-Async-Refresh` and the deferred task marks it `finished` with the fetched count. The local-root `finished` row is written after the response. Local run, 15-reply thread: 77 D1 queries. Follow-up: traversal still costs ~4 queries per node (replies, owner, visibility); a recursive CTE over local and remote replies would make it constant.
6. [x] **Conversations.** `GET /api/v1/conversations` loads participants, local accounts, remote actors, account stats, and last statuses for the whole page in two batched rounds and renders last statuses through `render_status_items`. Single-conversation renders (streaming, PATCH) keep `conversation_document`. Local run, 6 conversations: 23 D1 queries, independent of page size.
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
