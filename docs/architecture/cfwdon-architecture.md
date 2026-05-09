# cfwdon Architecture

## Summary

`cfwdon` is a Rust implementation of a Mastodon-compatible server designed for Cloudflare Workers. It uses D1 for relational state, R2 for media storage, Cloudflare Access for protected API authentication, and Worker-compatible request lifetimes for federation and API work.

The project borrows responsibility boundaries from GoToSocial, but it does not port GoToSocial directly. The architecture is shaped around Workers, D1, R2, cron/internal routes, and retryable delivery queues.

## Goals

- Run a Mastodon-compatible API server on Cloudflare Workers.
- Keep persistence in D1 and media bodies in R2.
- Support ActivityPub discovery, actor documents, inbox handling, outbox documents, signed delivery, and cached remote objects.
- Keep Mastodon API response builders, route surfaces, storage helpers, and federation transport separated enough to evolve independently.
- Preserve a path from the current Worker-heavy crate toward future application, federation, and storage crates.

## Non-Goals

- Directly port GoToSocial.
- Claim full behavioral Mastodon compatibility only because route-level coverage exists.
- Run long-lived background workers outside the primitives available to Cloudflare Workers.
- Implement a first-party OAuth authorization server while Cloudflare Access is the authentication boundary.

## Context

GoToSocial is useful as a responsibility map: HTTP API, processing, database, federation, media, routing, state, storage, and workers are distinct concerns. Cloudflare Workers change the runtime constraints: no long-lived process, no local filesystem, no direct TCP assumptions, and short request lifetimes.

`cfwdon` therefore keeps the Worker entrypoint thin where possible and pushes repeated concerns into focused internal modules. The current code still contains a large `cfwdon-worker` crate, but route, response, store, federation, notification, poll, media, and status responsibilities are increasingly split by capability.

The local `../rustresort` repository remains a reference for Mastodon API shape, federation flow, migrations, and e2e test ideas.

## Workspace

- `crates/cfwdon-core`
  Shared configuration, build metadata, and platform-neutral base types.
- `crates/cfwdon-domain`
  Domain types for accounts, statuses, media, and instance-oriented data.
- `crates/cfwdon-worker`
  Cloudflare Worker runtime, routing, D1/R2 bindings, Mastodon API surfaces, ActivityPub federation, internal jobs, and response/store modules.

Future extraction candidates remain `cfwdon-application`, `cfwdon-federation`, `cfwdon-storage-d1`, and `cfwdon-storage-r2`.

## Runtime Boundaries

- `router.rs` owns top-level route registration and connects HTTP surfaces to capability modules.
- `runtime_config.rs` reads Worker vars, build metadata, root document configuration, and upload limits.
- `auth.rs` handles Cloudflare Access JWT verification, header normalization, local account provisioning, and authenticated account lookup.
- `request_utils.rs`, `response_utils.rs`, `time_html.rs`, `db_utils.rs`, `id_utils.rs`, and `content_helpers.rs` provide shared request, response, time, query, ID, and content helpers.
- `instance_identity.rs` centralizes instance domain, actor URL, WebFinger, shared inbox, remote ID, and authority normalization.

## Mastodon API Modules

- `responses.rs` and related response modules own Mastodon DTO construction for accounts, statuses, media, reports, tags, search, context, and notifications.
- `profile.rs`, `account_store.rs`, `account_actions.rs`, `relationships.rs`, and account-related modules handle account reads, profile updates, relationship state, directory/search behavior, and follow/block/mute actions.
- `statuses.rs`, `status_store.rs`, `status_mutations.rs`, `status_interactions.rs`, and status-related modules handle status creation, reads, deletion, context, favourites, reblogs, bookmarks, pins, edits, quotes, translations, and visibility.
- `timeline_search.rs` owns public/home/tag/direct timelines, account/status/tag search, URL resolution, ranking, and tag response building.
- `notifications.rs` and `notification_routes.rs` own notification collection, visibility, filtering, dismiss/clear state, unread count, and grouped notification surfaces.
- `polls.rs` and poll modules own local and remote poll storage, votes, ActivityPub `Question` mapping, expired poll processing, and Mastodon poll responses.
- `media.rs` owns media upload, metadata update, profile media, fallback delivery, and orphan cleanup.
- `reports.rs`, `filters.rs`, `featured_tags.rs`, list/filter/push/meta modules, and placeholder routes cover broader Mastodon API surfaces.

## ActivityPub And Federation Modules

- `activitypub.rs` builds actor, note, question, update, delete, and audience/object helper shapes.
- `discovery.rs` serves WebFinger, actor/tag public reads, followers/following collections, and outbox documents.
- `inbox.rs` handles personal and shared inbox ingress, idempotency, target account resolution, and incoming activity dispatch.
- `delivery.rs` and delivery modules handle outbound activity rows, target fan-out, signed delivery, retry/backoff, terminal failure reconciliation, and follower delivery.
- `remote_objects.rs`, `remote_store.rs`, and federation cache helpers resolve and store remote actors, statuses, polls, and account references.
- `federation_http.rs` and HTTP signature modules handle signed ActivityPub delivery, inbox signature verification, remote document fetch, and SSRF-resistant URL validation.
- `crypto_keys.rs` owns RSA key generation, WebCrypto import/export, signature parameters, and public key PEM handling.

## Data Model

D1 stores local accounts, statuses, media metadata, follows, followers, blocks, mutes, favourites, bookmarks, notifications, polls, remote actors, remote statuses, inbox activity state, outbound activity/delivery state, reports, filters, featured tags, and instance settings.

R2 stores media bodies and profile media. D1 stores object keys, MIME metadata, description/focus metadata, and relationships to statuses or accounts.

The schema is migration-driven. Future work should add migration tests, seed data, and index reviews for large timelines, search, notifications, polls, and relationship queries.

## Authentication Model

Protected user-facing API routes rely on Cloudflare Access or an equivalent proxy. `cfwdon` validates the Access JWT, checks issuer/audience, reads the authenticated user e-mail, and maps that user to a local account.

Public routes and federation routes remain available without application login. ActivityPub routes perform HTTP signature and digest/date validation where required.

Because Cloudflare Access is the authentication boundary, OAuth client registration and token issuance are compatibility surfaces rather than a full internal authorization-server implementation.

## API And Federation Behavior

The Worker exposes Mastodon API v1/v2 routes, discovery/OAuth metadata routes, ActivityPub actor/status/outbox/followers/following routes, personal/shared inbox routes, media fallback routes, and internal cron/process routes.

Route-level Mastodon coverage is tracked in `docs/mastodon-api-compat/`. That inventory proves path/method coverage, not full behavioral parity. Behavioral compatibility must be verified with response-shape tests, e2e API tests, and federation interop tests.

ActivityPub delivery is queue-oriented. Local public/unlisted creates, deletes, interactions, profile updates, poll updates, and follow-related activities enqueue outbound work. Delivery rows are keyed to avoid duplicate target fan-out, and retry state is persisted in D1.

## Operational Plan

- Deploy as a single Cloudflare Worker.
- Configure D1 and R2 bindings in `wrangler.toml`.
- Use `INSTANCE_*`, `SOURCE_URL`, language, contact, thumbnail, policy, and media vars for public instance metadata.
- Keep `MEDIA_PUBLIC_BASE_URL` on a public media domain outside Cloudflare Access.
- Protect user and internal routes with Cloudflare Access settings where required.
- Use cron/internal routes for scheduled maintenance such as delivery processing, media pruning, and expired poll handling.

## Observability

The project should prefer structured JSON logs with request IDs, actor IDs, delivery targets, retry counts, and route context. Important event classes include D1 failures, R2 failures, remote fetch failures, signature verification failures, delivery retries, terminal delivery failures, and inbox replay decisions.

## Reliability And Failure Modes

- Missing D1/R2 bindings should fail with clear configuration errors.
- Media writes need recovery paths for partial R2/D1 success.
- Outbound delivery must remain idempotent across retries.
- Shared inbox processing must dedupe replayed activity IDs.
- Remote fetches must keep SSRF defenses active for actor, inbox, public key, and status URLs.
- Private visibility checks must stay centralized enough that API and ActivityPub reads agree.

## Security And Privacy

- Store API keys and private material as Cloudflare secrets or D1 data as appropriate; do not commit secrets.
- Keep public media outside Cloudflare Access while protecting private API and internal process routes.
- Validate Access JWT issuer and audience fail-closed.
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
- Implement OAuth internally: improves Mastodon client compatibility, but conflicts with the current Cloudflare Access-first operating model.

## Open Questions

- Which placeholder/meta routes should become real implementations first, and which should remain conservative empty responses?
- Which delivery work should move from `waitUntil` or internal routes to Cloudflare Queues?
- How should remote media caching and attachment persistence work long term?
- How much Mastodon OAuth compatibility should be provided without weakening the Cloudflare Access model?
- What operator-facing tooling is needed for retry dead-letter state, migrations, and moderation workflows?

## References

- GoToSocial repository
  `https://github.com/superseriousbusiness/gotosocial`
- Cloudflare Workers Rust support
  `https://developers.cloudflare.com/workers/languages/rust/`
- Local RustResort reference repository
  `../rustresort`
