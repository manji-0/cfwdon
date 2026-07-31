# Project Plan

This document is the current planning source for `cfwdon`. It replaces the older bootstrap-era running log with a status-oriented view: what is in place, what is intentionally limited, and what should happen next.

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

## Highest Priority Next Work

1. Expand behavioral compatibility tests beyond route presence, especially for placeholders that intentionally return empty or conservative responses.
2. Add federation interop tests for signed delivery, inbox replay behavior, remote polls, follow state transitions, and private visibility.
3. Improve remote media attachment handling, including cache policy, attachment normalization, and failure recovery.
4. Add migration tests and seed tooling so D1 schema changes are safer to review and deploy.
5. Harden operational controls around shared inbox abuse, signature clock skew, retry dead-letter inspection, and protected internal routes.

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
- Decide where Queues should replace `waitUntil` or internal cron routes for high-volume delivery.
- Track tombstones and soft deletes for remote objects more explicitly.

## Storage And Data Follow-Up

- Define a durable remote media attachment policy.
- Add migration tests and a repeatable D1 migration runner workflow.
- Add seed data for local Worker development.
- Decide how to expose retry dead-letter state to operators.
- Review indexes for timeline, notification, search, poll, and relationship queries as data grows.

## Security Follow-Up

- Add rate limiting and abuse controls for shared inbox and expensive remote resolution paths.
- Make signature clock skew policy configurable.
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
- D1 migration runner script.
- Structured logging with request, actor, delivery target, and retry metadata.
- Compatibility fixtures and e2e API tests.
- Federation interop tests.

## Durable Objects Follow-Up
<!-- derived-from durable-objects-candidates.md -->

Streaming already validates channels and serves SSE/WebSocket clients by polling D1 every few seconds, then recycling before Worker subrequest limits. Durable Objects are the main candidate to replace that poll loop with hibernatable WebSocket hubs and write-time fan-out. See [Durable Objects Candidates](durable-objects-candidates.md) for ranking, sharding atoms, and a phased spike plan.

- Spike a `StreamHub` DO for one authenticated channel (`user` or `user:notification`) with hibernation.
- Publish prebuilt Mastodon streaming payloads after D1 commits; keep D1 as source of truth.
- Evaluate keyed DO rate limiters for shared inbox / remote fetch abuse separately from streaming.
- Prefer per-remote-host **admission** DOs behind the existing shared/personal inbox URLs; do not invent per-host public inbox paths.
- Keep outbound ActivityPub delivery on Queues; do not move fan-out HTTP delivery into DOs.
