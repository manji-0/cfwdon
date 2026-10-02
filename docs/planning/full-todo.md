# Project Plan

This document is the current planning source for `cfwdon`. It replaces the older bootstrap-era running log with a status-oriented view: what is in place, what is intentionally limited, and what should happen next.

**Status:** current tracker. Items marked **Done** have landed (evidence in `git log`); unmarked items are open unless labelled deferred.

## Principles

- Keep the GoToSocial-inspired responsibility split, but adapt the implementation to Workers, D1, R2, and short request lifetimes.
- Use `../rustresort` as a reference for API shapes and federation boundaries, not as a direct porting source.
- Prefer correct signatures, visibility, ownership checks, and idempotency over shallow endpoint coverage.
- Treat the generated Mastodon compatibility inventory as a route map, not as proof that every route is behaviorally complete.

## Current Baseline
<!-- derived-from ../mastodon-api-compat/README.md -->

The generated compatibility inventory currently maps all tracked upstream routes to local handlers, with no route-level `missing` or `compat-gap` entries. The remaining work is behavioral compatibility, operational hardening, data model completeness, and interop testing.

## Completed Capability Areas

- Rust workspace, `devbox` development environment, CI gate, and Worker dry-run validation.
- Auth0 authentication, JWT validation, local account provisioning, and protected route checks.
- D1-backed local accounts, statuses, relationships, notifications, polls, reports, filters, featured tags, and instance metadata.
- R2-backed media upload, media metadata update, profile avatar/header storage, and media delivery fallback.
- WebFinger, ActivityPub actor documents, followers/following collections, outbox documents, and local status objects.
- Personal and shared inbox handling for follow, undo, accept, reject, create, update, delete, like, announce, and poll vote activity slices.
- Signed outbound delivery, targeted activity queues, retry/backoff, terminal failure reconciliation, and idempotency safeguards.
- Local and remote timeline/search/status surfaces, including public/home/tag/direct timelines, context, cards, history, favourites, bookmarks, mutes, blocks, and pins.
- Mastodon-compatible instance v1/v2, app, OAuth metadata, notification, report, list, filter, push, suggestion, trend, announcement, donation, and placeholder surfaces.
- Local and remote poll support, including ActivityPub `Question` federation, votes, vote undo, own-vote remapping, and expired poll closure updates.
- DNS-based SSRF defense for remote fetch targets and cached remote actor key use during signature verification.
- Generated Mastodon API route inventory and response-shape compatibility tests for important DTOs.
- `StreamHub` Durable Object streaming for every channel (see [Durable Objects Follow-Up](#durable-objects-follow-up)).
- D1 Sessions API on read-heavy routes (timelines, notifications, status detail, instance) with `x-d1-bookmark` continuity; see [D1 Sessions API Feasibility Spike](d1-sessions-api-spike.md).
- Cron-driven sweeps: expired polls, due scheduled statuses, and the outbox (`crons` in `wrangler.toml.example`).
- Embedded admin API and UI (`/admin`, `/api/cfwdon/admin/*`) with Auth0 role authorization: dashboard, report resolution, delivery inspection and retry, relays, domain blocks, and custom emoji.
- Registered custom emoji registry (D1/R2) with federated `Emoji` tag read support.
- Vite SPA web UI on TanStack Router (see [Web UI Follow-Up](#web-ui-follow-up)).
- CI guards for D1 migrations (`scripts/check_migrations.py`), query plans (`scripts/check_query_plans.py`), and worker module layers (`scripts/check_module_layers.py`).

## Highest Priority Next Work

1. Expand behavioral compatibility tests beyond route presence, especially for placeholders that intentionally return empty or conservative responses.
2. Add federation interop tests for signed delivery, inbox replay behavior, remote polls, follow state transitions, and private visibility. Fixture-level Misskey coverage exists (`activitypub/misskey_compat_tests.rs`); live round-trips against real implementations are still open.
3. Improve remote media attachment handling, including cache policy, attachment normalization, and failure recovery.
4. Add seed tooling so local Worker development starts with data. **Migration validation is done:** `scripts/check_migrations.py` runs in `devbox run ci`, and `scripts/run_worker_dev.mjs` applies pending local migrations.
5. Harden operational controls around shared inbox abuse, signature clock skew, and protected internal routes. **Retry dead-letter inspection is done** via the admin deliveries API (`admin_api/deliveries.rs`).

## Mastodon API Follow-Up

- Verify whether extra routes are deprecated Mastodon routes, deliberate compatibility aliases, or local-only extensions.
- Improve behavioral parity for notification grouping, filters, lists, follow requests, suggestions, trends, and WebPush delivery.
- Review placeholder/meta routes and document which are intentionally empty, read-only, or minimally implemented.
- Expand private/remote permission checks for polls, conversations, timelines, media, and account/status collections.
- Keep `docs/mastodon-api-compat/` regenerated whenever `crates/cfwdon-worker/src/router.rs` changes.

## ActivityPub Follow-Up

- Test Create/Update/Delete/Like/Announce/Follow/Accept/Reject flows against real federated implementations.
- Track Misskey ActivityPub interop gaps and residual live tests in [Misskey ActivityPub Interop](misskey-activitypub-interop.md).
- Improve remote `Question` update handling, option rename detection, and vote refresh semantics.
- Add stronger replay and dedupe coverage for shared inbox traffic.
- Outbound delivery already runs on Queues (`OUTBOX_PROCESS_QUEUE` producer and consumer in `wrangler.toml.example`). **Deferred:** an `INBOX_PROCESS_QUEUE` handoff until a D1 staging table exists (`33110ff`; see [Durable Objects Candidates](durable-objects-candidates.md)). Remaining `waitUntil` and internal cron routes (`/internal/*`) stay until measured under load.
- Track tombstones and soft deletes for remote objects more explicitly.

## Storage And Data Follow-Up

- Define a durable remote media attachment policy.
- **Done:** migration validation (`scripts/check_migrations.py` in CI) and a repeatable local migration workflow (`run_worker_dev.mjs` applies pending migrations; remote uses `wrangler d1 migrations apply`).
- Add seed data for local Worker development.
- **Done:** expose retry dead-letter state to operators: `GET /api/cfwdon/admin/deliveries` (state filter) and `POST /api/cfwdon/admin/deliveries/:id/retry` for failed rows.
- Review indexes for timeline, notification, search, poll, and relationship queries as data grows.

## Security Follow-Up

- Add rate limiting and abuse controls for shared inbox and expensive remote resolution paths.
- Make signature clock skew policy configurable (it is currently the fixed `ACTIVITYPUB_MAX_DATE_SKEW_MS` constant, 12 hours, in `cfwdon-domain`).
- Harden digest and signed-header canonicalization tests.
- Audit all internal routes and document which must require Auth0 authentication.
- Keep public media domains outside protected API authentication while preserving cache behavior.

## Media Delivery Notes

- The fallback `/media/:id` route returns an R2 object through the Worker and should not be treated as guaranteed main-request edge cache coverage.
- Prefer an R2 custom domain plus Cache Rules or a fetch-based public path for canonical media delivery.
- Keep public media outside protected API authentication so media cache behavior stays predictable.
- Entity payloads should continue to advertise `MEDIA_PUBLIC_BASE_URL` as the canonical media base.

## Ops / DX Follow-Up

- `wrangler dev` seed script.
- ~~D1 migration runner script.~~ **Done:** `devbox run worker:dev` applies pending local migrations.
- Structured logging with request, actor, delivery target, and retry metadata.
- Compatibility fixtures and e2e API tests.
- Federation interop tests.

## Web UI Follow-Up
<!-- derived-from web-ui-routing.md -->

The Vite SPA under `/app` now uses TanStack Router in SPA library mode with a code-based route tree. See [Web UI Routing Modernization](web-ui-routing.md) for the inventory, rejected full-stack options, and later optional polish.

- Finish `AppRoute` coverage for status, profile-by-id, collection, and search-query paths before swapping libraries. **Done:** `AppRoute` is the path helper; `router.tsx` uses `AppRoute.path`.
- Spike TanStack Router in SPA library mode. **Done:** the full tree moved with the spike (two routers cannot coexist); `react-router` is gone; `ViewCache` and Worker `/app` fallback are unchanged.
- Tighten typed `Link` / `navigate` without adapter casts. **Done:** `matchAppLinkTarget` plus a discriminated `AppLinkTarget`; `AppRoute` stays the domain helper.
- Do not adopt React Router Framework Mode, TanStack Start, or a JS SSR runtime next to the Rust Worker.

## Durable Objects Follow-Up
<!-- derived-from durable-objects-candidates.md -->

Streaming is served by the hibernating `StreamHub` Durable Object for every channel (per-account session hubs for authenticated channels, shared hubs for public/hashtag); the D1 poll loop remains only as a fallback and 30s catch-up. See [Durable Objects Candidates](durable-objects-candidates.md) for ranking, sharding atoms, and phase status.

- **Done:** `StreamHub` DO with hibernation, covering `user`, `user:notification`, `list`, `direct`, public and hashtag channels.
- **Done:** Publish prebuilt Mastodon streaming payloads after D1 commits; D1 stays the source of truth.
- Deferred until measured: public-channel hash sharding (a single public hub is far below the ~500–1000 msgs/sec ceiling).
- Deferred until measured: evaluate keyed DO rate limiters for shared inbox / remote fetch abuse separately from streaming.
- Deferred until single-host inbox pressure is observed: per-remote-host **admission** DOs behind the existing shared/personal inbox URLs (the `InboxHost` spike was reverted); do not invent per-host public inbox paths.
- Open: raise or queue the follower fan-out cap (`STREAM_HUB_FOLLOWER_FANOUT_LIMIT`, 200) before running instances with larger follower counts.
- Keep outbound ActivityPub delivery on Queues; do not move fan-out HTTP delivery into DOs.
- Reserve the Agents SDK for optional ops/AI/MCP/email session products, not for Mastodon streaming or inbox wire paths.
