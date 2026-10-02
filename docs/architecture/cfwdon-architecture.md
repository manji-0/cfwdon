# cfwdon Architecture

## Summary

`cfwdon` is a Rust implementation of a Mastodon-compatible server designed for Cloudflare Workers. It uses D1 for relational state, R2 for media storage, KV for short-lived caches, a Durable Object (`StreamHub`) for streaming fan-out, a Queue for outbound delivery, Auth0 for protected API authentication, and Worker-compatible request lifetimes for federation and API work.

The project borrows responsibility boundaries from GoToSocial, but it does not port GoToSocial directly. The architecture is shaped around Workers, D1, R2, KV, Durable Objects, Queues, cron/internal routes, and retryable delivery.

## Goals

- Run a Mastodon-compatible API server on Cloudflare Workers.
- Keep persistence in D1 and media bodies in R2.
- Support ActivityPub discovery, actor documents, inbox handling, outbox documents, signed delivery, and cached remote objects.
- Keep Mastodon API response builders, route surfaces, storage helpers, and federation transport separated enough to evolve independently.
- Preserve a path from the current Worker-heavy crate toward future application, federation, and storage crates.
- Keep Cloudflare-specific bindings in the Worker crate; shared rules live in `cfwdon-domain` and are checked by Stateright models.

## Non-Goals

- Directly port GoToSocial.
- Claim full behavioral Mastodon compatibility only because route-level coverage exists.
- Run long-lived background workers outside the primitives available to Cloudflare Workers.
- Implement a first-party OAuth authorization server while Auth0 is the authentication boundary.

## Context

GoToSocial is useful as a responsibility map: HTTP API, processing, database, federation, media, routing, state, storage, and workers are distinct concerns. Cloudflare Workers change the runtime constraints: no long-lived process, no local filesystem, no direct TCP assumptions, and short request lifetimes.

`cfwdon` therefore keeps the Worker entrypoint thin where possible and pushes repeated concerns into focused internal modules. The current code still contains a large `cfwdon-worker` crate, but route, response, store, federation, notification, poll, media, and status responsibilities are increasingly split by capability.

The local `../rustresort` repository remains a reference for Mastodon API shape, federation flow, migrations, and e2e test ideas.

## Workspace

- `crates/cfwdon-core`
  Shared configuration, build metadata, and platform-neutral base types.
- `crates/cfwdon-domain`
  Domain types for accounts, statuses, media, and instance-oriented data.
- `crates/cfwdon-models`
  Stateright models and refinement checks for domain state machines; see [Model Refinement Mapping](../reference/model-refinement.md).
- `crates/cfwdon-worker`
  Cloudflare Worker runtime, routing, D1/R2 bindings, Mastodon API surfaces, ActivityPub federation, internal jobs, and response/store modules.

Future extraction candidates remain `cfwdon-application`, `cfwdon-federation`, `cfwdon-storage-d1`, and `cfwdon-storage-r2`.

## Runtime Boundaries

- `router.rs` owns top-level route registration and connects HTTP surfaces to capability modules.
- `runtime_config.rs` reads Worker vars and secrets, build metadata, root document configuration, and upload limits. Binding names and vars are listed in [Configuration Reference](../reference/configuration.md).
- `auth` handles Auth0 JWT verification, local account provisioning, and authenticated account lookup.
- `request_utils.rs`, `response_utils.rs`, `time_html.rs`, `db_utils.rs`, `id_utils.rs`, and `content_helpers.rs` provide shared request, response, time, query, ID, and content helpers.
- `identity.rs` centralizes instance domain, actor URL, WebFinger, shared inbox, remote ID, and authority normalization.
- `db_session.rs` opens a request-scoped D1 Sessions API session: `GET`/`HEAD` start `first-unconstrained` so reads can use read replicas, mutating methods start `first-primary`, and an `x-d1-bookmark` request header continues an earlier session. It is wired into selected read-heavy routes (timelines, notifications, status detail); see [D1 Sessions API Spike](../planning/d1-sessions-api-spike.md).
- `stream_hub/` is the `StreamHub` Durable Object (bound as `STREAM_HUB`) behind WebSocket and SSE streaming; `stream_hub_publish.rs` publishes events inline after D1 commits.
- Queue `OUTBOX_PROCESS_QUEUE` drains outbound ActivityPub delivery; the `#[event(queue)]` consumer is in `lib.rs` and the producer in `delivery.rs`.
- KV `REMOTE_DNS_CACHE` caches DoH SSRF validation results; KV `APP_CACHE` (`app_cache.rs`) caches account capability bits and public endpoint/trend payloads. `response_cache.rs` uses the Workers Cache API for anonymous public responses (`[cache]` in `wrangler.toml.example`).
- `[placement]` pins the Worker next to the D1 primary, and `[observability]` enables logs and traces; `observability.rs` and `d1_metrics.rs` emit structured events and per-request D1 metrics. See [Configuration Reference](../reference/configuration.md#worker-and-d1-placement).
- `web_ui/`, `admin_ui/`, and `ui_assets.rs` serve the staged UI builds from the `ASSETS` binding.

## Mastodon API Modules

- `responses.rs` and related response modules own Mastodon DTO construction for accounts, statuses, media, reports, tags, search, context, and notifications.
- `profile/`, `accounts/`, `relationships.rs`, `relationship/`, and account-related modules handle account reads, profile updates, relationship state, directory/search behavior, and follow/block/mute actions.
- `statuses/` and status-related modules handle status creation, reads, deletion, context, favourites, reblogs, bookmarks, pins, edits, quotes, translations, and visibility.
- `timelines/` and `search/` own public/home/tag/link/direct timelines, account/status/tag search, URL resolution, ranking, and tag response building.
- `notifications/` owns notification collection, visibility, filtering, dismiss/clear state, unread count, and grouped notification surfaces.
- `polls/`, `local_polls/`, and poll modules own local and remote poll storage, votes, ActivityPub `Question` mapping, expired poll processing, and Mastodon poll responses.
- `media/` owns media upload, metadata update, profile media, fallback delivery, and orphan cleanup.
- `reports/`, `filters/`, `featured_tags.rs`, list/filter/push/meta modules, and placeholder routes cover broader Mastodon API surfaces.

## ActivityPub And Federation Modules

- `activitypub/` builds actor, note, question, update, delete, and audience/object helper shapes.
- `discovery/` serves WebFinger, actor/tag public reads, followers/following collections, and outbox documents.
- `inbox/` handles personal and shared inbox ingress, idempotency, target account resolution, and incoming activity dispatch.
- `delivery/` handles outbound activity rows, target fan-out, signed delivery, retry/backoff, terminal failure reconciliation, and follower delivery.
- `remote/` and `federation/` cache helpers resolve and store remote actors, statuses, polls, and account references.
- `federation/` and `http/` (signatures, request validation, signed delivery) handle signed ActivityPub delivery, inbox signature verification, remote document fetch, and SSRF-resistant URL validation.
- `crypto_keys.rs` owns RSA key generation, WebCrypto import/export, signature parameters, and public key PEM handling.

## Data Model

D1 stores local accounts, statuses, media metadata, follows, followers, blocks, mutes, favourites, bookmarks, notifications, polls, remote actors, remote statuses, inbox activity state, outbound activity/delivery state, reports, filters, featured tags, and instance settings.

R2 stores media bodies and profile media. D1 stores object keys, MIME metadata, description/focus metadata, and relationships to statuses or accounts.

The schema is migration-driven. Future work should add migration tests, seed data, and index reviews for large timelines, search, notifications, polls, and relationship queries.

## Authentication Model
<!-- derived-from ../reference/configuration.md#auth0-authentication-vars -->

Protected user-facing API routes rely on Auth0-issued JWTs. `cfwdon` validates the Auth0 JWT, checks issuer/audience, reads the configured e-mail claim, and maps that user to a local account.

Public routes and federation routes remain available without application login. ActivityPub routes perform HTTP signature and digest/date validation where required.

Because Auth0 is the authentication boundary, OAuth client registration and token issuance are compatibility surfaces rather than a full internal authorization-server implementation.

## API And Federation Behavior

The Worker exposes Mastodon API v1/v2 routes, discovery/OAuth metadata routes, ActivityPub actor/status/outbox/followers/following routes, personal/shared inbox routes, media fallback routes, and internal cron/process routes.

Route-level Mastodon coverage is tracked in `docs/mastodon-api-compat/`. That inventory proves path/method coverage, not full behavioral parity. Behavioral compatibility must be verified with response-shape tests, e2e API tests, and federation interop tests.

ActivityPub delivery is queue-oriented. Local public/unlisted creates, deletes, interactions, profile updates, poll updates, and follow-related activities enqueue outbound rows in D1. A Cloudflare Queue (`OUTBOX_PROCESS_QUEUE`) is kicked after successful write requests, from `POST /internal/outbox/process`, and from the hourly cron when work is pending; the queue consumer then fans out and delivers. Delivery rows are keyed to avoid duplicate target fan-out, and retry state is persisted in D1.

## Operational Plan
<!-- constrained-by ../reference/configuration.md#cloudflare-bindings -->
<!-- constrained-by ./web-ui-foreground-resume.md -->

- Deploy as a single Cloudflare Worker.
- Attach Vite `web-ui` (React) and `admin-ui` (Svelte) builds as Workers static assets under `/app` and `/admin`.
- The `/app` SPA reconnects streaming and REST-catches up after a long-backgrounded tab returns; see [Web UI Foreground Resume](web-ui-foreground-resume.md).
- Configure the D1, R2, KV, Queue, and Durable Object bindings in `wrangler.toml`.
- Use `INSTANCE_*`, `SOURCE_URL`, language, contact, thumbnail, policy, and media vars for public instance metadata.
- Keep `MEDIA_PUBLIC_BASE_URL` on a public media domain.
- Protect user and internal routes with Auth0 settings where required.
- Two cron triggers run maintenance: hourly (outbox queue kick, expired polls, scheduled statuses, background jobs such as card unfurls, stale inbox reclaim, remote content retention, cache refresh) and every six hours (trending tags and statuses). Orphaned media is pruned through `POST /internal/media/prune-orphans`, not cron.

## Observability

`[observability]` logs and traces are enabled in `wrangler.toml.example`. The project prefers structured JSON logs with request IDs, actor IDs, delivery targets, retry counts, and route context. Important event classes include D1 failures, R2 failures, remote fetch failures, signature verification failures, delivery retries, terminal delivery failures, and inbox replay decisions.

## Reliability And Failure Modes

- Missing D1/R2 bindings should fail with clear configuration errors.
- Media writes need recovery paths for partial R2/D1 success.
- Outbound delivery must remain idempotent across retries.
- Shared inbox processing must dedupe replayed activity IDs.
- Remote fetches must keep SSRF defenses active for actor, inbox, public key, and status URLs.
- Private visibility checks must stay centralized enough that API and ActivityPub reads agree.

## Security And Privacy

- Store API keys and private material as Cloudflare secrets or D1 data as appropriate; do not commit secrets.
- Keep public media outside the protected API surface.
- Validate Auth0 JWT issuer and audience fail-closed.
- Validate incoming ActivityPub signatures, dates, and digests fail-closed for signed inbox traffic.
- Maintain DNS-based SSRF checks for remote resolution paths.
- Treat private/direct status visibility as a cross-cutting data access concern.

## Rollout History

The initial phases were:

- Phase 0: Cargo workspace, Worker entrypoint, and design docs.
- Phase 1: D1 schema, instance information, account creation, and local status creation.
- Phase 2: R2 media, WebFinger, ActivityPub actor/object output.
- Phase 3: inbox/outbox, signed delivery, follow relationships, and home/public timelines.
- Phase 4: broader Mastodon API coverage, compatibility inventory, notifications, polls, reports, filters, search, and operational surfaces.

The project is now past the bootstrap phases. The planning focus is behavioral compatibility, interop testing, operational hardening, and data model durability.

## Alternatives Considered

- Single crate for everything: quick to start, but it becomes harder to maintain as Mastodon API and ActivityPub coverage grows.
- Many crates from day one: cleaner dependency boundaries, but high early maintenance cost.
- Let Cloudflare-specific types leak into domain code: convenient initially, but weakens tests and portability.
- Implement OAuth internally: improves Mastodon client compatibility, but conflicts with the current Auth0-first operating model.

## Open Questions

- Which placeholder/meta routes should become real implementations first, and which should remain conservative empty responses?
- Outbound delivery already runs on Cloudflare Queues, and no code path uses `waitUntil`. Card unfurls, remote context fetches, and similar jobs use the D1 `background_jobs` table drained by the hourly cron; should any of them move to Queues?
- Streaming channels are already served by the `StreamHub` Durable Object, with D1 polling kept only as a fallback. Which other surfaces should use Durable Objects? See [Durable Objects Candidates](../planning/durable-objects-candidates.md).
- How should remote media caching and attachment persistence work long term?
- How much Mastodon OAuth compatibility should be provided without weakening the Auth0 model?
- What operator-facing tooling is needed for retry dead-letter state, migrations, and moderation workflows?

## References

- GoToSocial repository
  `https://github.com/superseriousbusiness/gotosocial`
- Cloudflare Workers Rust support
  `https://developers.cloudflare.com/workers/languages/rust/`
- Local RustResort reference repository
  `../rustresort`
