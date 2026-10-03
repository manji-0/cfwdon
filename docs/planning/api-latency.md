# API Latency Reduction

**Status:** items 1–12 implemented on branch `perf/api-latency`; production observation pending.

Goal: cut the serial data round trips (D1, KV, Cache API, Durable Object, outbound HTTP) on the critical path of the main Mastodon API scenarios, without changing response shapes. Every item ends with `devbox run ci` green and one conventional commit.

## Context

<!-- constrained-by ../architecture/cfwdon-architecture.md#runtime-boundaries -->
<!-- derived-from ./d1-sessions-api-spike.md -->

The Worker is placed next to the D1 primary (`[placement] region`), so one D1 round trip is short and latency scales with the number of *serial* round trips. Read replicas (Sessions API) add little latency benefit under this placement; they are out of scope until the round-trip counts drop.

## Ordered Work Items

1. [x] **Deferred work via `ctx.wait_until`.** `deferred.rs` queues owned futures; `fetch` drains them under `Context::wait_until` (scheduled and queue handlers drain inline). Deferred: the outbox queue kick (`router.rs`), Web Push sends, StreamHub fan-out for status create/edit/delete and notifications, Cache API puts, public-endpoint KV puts. Kept awaited on purpose: Cache API deletes (clients re-read right after a profile write; the three deletes now run concurrently) and the account-capabilities KV put (a late put could outlive an invalidation). Local run: POST /statuses 48 → 25 D1 queries before the response, favourite 35 → 19, reblog 46 → 30.
2. [x] **Custom emoji cache.** `custom_emojis/store.rs` keeps the table rows in an isolate L1 with a 60 s TTL, cleared on admin create/update/delete in the same isolate. KV was skipped: with the Worker next to D1 a KV read saves little, and a second cache layer widens the stale window after admin edits.
3. [x] **List timeline.** `timelines/list_timeline.rs` resolves members (account id, `user@domain`, actor URI) in SQL and seeks each member's public statuses, then renders through the home timeline's shared `timeline_entries_from_candidate_rows`. Paging now reaches statuses older than the public-timeline window, and filters and muted authors apply as on home. Local run: 145 → 33 D1 queries.
4. [x] **Notifications.** Follow and follow-request collectors load their local/remote rows concurrently and hydrate accounts, actors, and notification mutes in one batched round (was three serial queries per row); dismissals and the clear marker load alongside the collectors. Mute reads no longer `DELETE` expired rows (they filter on `expires_at`), and the hourly cron calls `purge_expired_mutes`. Admin report/sign-up collectors stay per-row (admin-only, low volume). Local run with 30 follow notifications: 50 D1 queries, independent of the follower count.
5. [x] **Status context.** Local and remote context traversal now collects nodes and renders them once through `timelines::render_status_items` (the timeline preload path) instead of a full status build per node. For signed-in viewers the remote reply-tree fetch (`hydrate_remote_descendants_for_context`, outbound HTTP + upserts) runs after the response; the context answers with a `running` `Mastodon-Async-Refresh` and the deferred task marks it `finished` with the fetched count. Hydration stops starting new fetches after a 20 s budget so it can close the refresh inside `wait_until`, and a `running` row older than 60 s reads as `finished` in case the isolate is evicted first. The local-root `finished` row is written after the response. Local run, 15-reply thread: 77 D1 queries. Follow-up: traversal still costs ~4 queries per node (replies, owner, visibility); a recursive CTE over local and remote replies would make it constant.
6. [x] **Conversations.** `GET /api/v1/conversations` loads participants, local accounts, remote actors, account stats, and last statuses for the whole page in two batched rounds and renders last statuses through `render_status_items`. Single-conversation renders (streaming, PATCH) keep `conversation_document`. Local run, 6 conversations: 23 D1 queries, independent of page size.
7. [x] **Account statuses and status detail.** `GET /api/v1/accounts/:id/statuses` (JSON) checks visibility and account filters for the page concurrently, then renders survivors with `render_status_items`; it previously built each status one `await` at a time with per-status mention, boost, and emoji lookups. `GET /api/v1/statuses/:id` renders through the same path, so its lookups run as parallel preload rounds instead of a serial chain (the query count stays about the same).
8. [x] **POST /statuses.** `insert_status` returns the in-memory status (it carries every column the INSERT wrote) instead of re-reading it, and runs the outbox enqueues, hashtag, mention, poll, and card-job writes concurrently. Hashtag, mention, and poll rows are each written as one `db.batch` (also making the mention replace atomic). The duplicate viewer-agnostic stream payload build already moved off the response path in item 1. Verified: the POST response equals a follow-up GET; edits still add/remove tags and mentions.
9. [x] **Favourite / reblog / bookmark lists and actions.** Action responses (favourite, reblog, bookmark, pin, and their undo) render through `render_status_items`; the reblog wrapper is no longer re-read after it is written. `enqueue_targeted_outbox_activity` (Announce, Delete, and other per-inbox fan-out) writes all inbox rows in one `db.batch` instead of one INSERT per follower inbox. `GET /api/v1/bookmarks` and `/api/v1/favourites` load statuses by id in bulk, check visibility concurrently, and render once (they built each status serially). Follow/unfollow were left as is: about six serial queries, with the recipient notification already deferred in item 1.
10. [x] **Remote account reads.** `GET /api/v1/accounts/:id` (remote) and `/api/v1/accounts/lookup` (remote handle) cache the origin-fetched account response in the Cache API for 5 minutes keyed by actor URI, so repeat views skip the WebFinger and actor fetches. Serving the stored row instead was rejected: it lacks profile fields and custom emojis, which only come from the fetched document. Only fresh fetches are cached (the stored-row fallback retries the origin next time); a profile `Update` is visible after at most the TTL. Exact-handle search still fetches synchronously. Not exercised locally (it needs live remote origins); covered by build and clippy only.
11. [x] **Inbox Create.** A shared-inbox delivery with K local recipients stored the remote actor and status K times (about 15 D1 round trips each). `Create` and status `Update` now check poll votes and addressing per recipient but upsert once; `Accept`, `Reject`, `Delete`, `Add`, and `Remove` (handlers that ignore the recipient) dispatch once. Actor `Update`, `Follow`, `Undo`, `Like`, and `Announce` keep the per-recipient loop. Not exercised locally (needs signed deliveries); covered by build, clippy, and unit tests.
12. [x] **Small items.** On a hashed-token miss, the legacy user-token and app-token lookups run concurrently (four serial lookups became three rounds). The legacy plaintext path selected a NULL `access_token` and answered 500; it now reports the hash it migrates to. Markers load/save both scopes concurrently. `GET /api/v1/accounts/:id` checks the response cache before authenticating and loads stats, settings, and emojis concurrently.

## Measurements

<!-- derived-from #ordered-work-items -->

Local `wrangler dev` (release build) with every D1 call delayed by 10 ms to stand in for network round trips. `main` (`09b9926`) and this branch ran the same scenario script from the same database snapshot (30 followers, a 15-reply thread, 6 direct conversations). Writes were followed by a 3 s pause so deferred work did not land in the next request. `q` is D1 queries counted before the response; `ms` is handler time.

| Request | q main | q branch | ms main | ms branch |
| --- | ---: | ---: | ---: | ---: |
| `POST /api/v1/statuses` (tag + mention) | 60 | 33 | 703 | 284 |
| `POST /api/v1/statuses/:id/favourite` | 35 | 19 | 446 | 123 |
| `POST /api/v1/statuses/:id/reblog` | 46 | 33 | 591 | 260 |
| `GET /api/v1/timelines/home` | 48 | 51 | 177 | 173 |
| `GET /api/v1/timelines/list/:id` | 574 | 42 | 7212 | 150 |
| `GET /api/v1/statuses/:id` | 17 | 17 | 215 | 94 |
| `GET /api/v1/statuses/:id/context` (15-reply root) | 292 | 77 | 3699 | 710 |
| `GET /api/v1/accounts/:id/statuses` | 56 | 40 | 609 | 154 |
| `GET /api/v1/bookmarks` | 162 | 45 | 2053 | 114 |
| `GET /api/v1/notifications` (30 follows) | 144 | 50 | 1241 | 106 |
| `GET /api/v1/conversations` (6) | 98 | 23 | 1235 | 91 |
| `GET /api/v1/accounts/verify_credentials` | 7 | 6 | 43 | 29 |

Home gains three queries from the boost-target viewer-state fix (`030d4e9`) at unchanged time. Remote account caching (item 10) and inbox deduplication (item 11) need live remote origins and signed deliveries and are not in this table.

## Caveats

- Deferred tasks run after `reset_d1_request_metrics`, so their D1 queries are counted on whichever request runs next on the isolate. That feeds the prior-request `sql_ms` load-shed heuristic in `d1_metrics.rs` (used by instance and public timeline); if it trips in production, record deferred queries under a separate bucket.

## Done Criteria

- [x] Every item above is checked or explicitly marked deferred with a reason.
- [x] `devbox run ci` passes on the branch head; new or changed hot SQL is covered by `scripts/check_query_plans.py`.
- [x] No routes were added or removed, so `docs/mastodon-api-compat/` was not regenerated. (The generator currently fails on an upstream route it cannot group, `GET /api/accounts/:account_id/collections`; that is unrelated to this branch.)
