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
- `db_session.rs` opens a request-scoped D1 Sessions API session: `GET`/`HEAD` start `first-unconstrained` so reads can use read replicas, mutating methods start `first-primary`, and an `x-d1-bookmark` request header continues an earlier session. It is wired into selected read-heavy routes (timelines, notifications, status detail). Session handles keep the primary binding, so a timeline `max_id`/`min_id`/`since_id` lookup that misses on a lagging replica retries on the primary (streaming can deliver a status before replicas have it) and logs `timeline_cursor_replica_miss`; see [D1 Sessions API Spike](../planning/d1-sessions-api-spike.md).
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

Failed deliveries follow Mastodon's retry policy (`cfwdon_domain::delivery`). A 2xx is delivered. A 4xx other than 401, 408 and 429, or a 501, fails the row at once. Anything else is retried with Sidekiq's backoff (`count^4 + 15` seconds plus jitter) for up to 17 attempts, about two days. On a 429 or 503 the remote's `Retry-After` (delta-seconds or HTTP-date) replaces the backoff when it is longer, capped at 12 hours.

### Instance Actor

The instance actor is Mastodon's representative `Application` at `/actor`. Its `preferredUsername` is the instance domain, and WebFinger resolves `acct:<domain>@<domain>` to it. Its key pair is generated on first use and stored in the one-row `instance_actor_keys` table. The private key is encrypted with `ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY` when that secret is set. `/actor/inbox` is handled as the shared inbox, and `/actor/outbox` is always empty.

Every signed ActivityPub GET is signed as the instance actor (`keyId` `/actor#main-key`), whether a background job or a user request triggered it. No local user's key signs a fetch, so a remote that blocked one user still answers, and the remote does not learn which user caused the fetch. Outbound deliveries are still signed by the account that sent the activity.

### Inbox Claims

Personal and shared inbox deliveries claim the activity id in `inbox_activities` before they apply it. The claim is one D1 batch, so one transaction: it drops a claim older than 15 minutes, inserts a fresh claim if none is held, and reads the row it collided with. The claim has three outcomes (`cfwdon_domain::InboxClaim`):

- `Acquired`: this delivery applies the activity. Success marks the row processed. A failure releases the row and answers an error, so the sender retries.
- `AlreadyProcessed`: the activity was applied before. The Worker answers `202` and logs `inbox_replay_skipped`.
- `InFlight`: another delivery holds the claim and has not finished. The Worker answers `503` with `Retry-After: 60` and logs `inbox_activity_in_flight`. A `2xx` here would make the sender stop. If the first apply was cut off (Worker timeout or eviction), nobody would then deliver the activity again, and the activity would be lost.

A claim left by an apply that was cut off goes stale after 15 minutes. The next retry takes the claim over and applies the activity. The hourly cron also reconciles stale claims: it marks a claim processed when the activity's effect is visible and releases a `Create` claim when it is not. Relay deliveries do not claim.

### Instance Domain Blocks

Admins manage instance domain blocks through Mastodon's `/api/v1/admin/domain_blocks` (scopes `admin:read:domain_blocks` and `admin:write:domain_blocks`) or the admin UI's `/api/cfwdon/admin/domain-blocks`. A block covers the domain and every subdomain: a block on `example.com` also covers `a.example.com`. When several blocks cover one host, each restriction applies if any of them imposes it. Each block carries Mastodon's `DomainBlock` fields:

- `severity: "suspend"` refuses all federation. The Worker refuses inbox deliveries signed by or attributed to a suspended host with `403` before any key fetch. It skips outbound deliveries to a suspended host as terminal failures, and `url_guard` refuses remote fetches to one. A suspension also implies every restriction below. Rows that existed before severities were added stay `suspend`.
- `severity: "silence"` hides the server's remote posts from the public and hashtag timelines, and a follow from one of its accounts waits for approval as if the local account were locked. Federation still works. Unlike Mastodon, a viewer who follows a silenced account does not see its posts on those timelines either, and silenced accounts' notifications and search results are not filtered.
- `severity: "noop"` limits nothing on its own and carries only the flags below.
- `reject_media` stores no attachments for the server's remote posts and no avatar or header for its accounts. The Worker never downloads remote media, so this keeps the server's media URLs out of D1 and out of API responses. Rows stored before the block keep their media until the post or profile is refreshed.
- `reject_reports` is stored and returned. The inbox does not process inbound `Flag` activities from any server, so there is nothing further to drop.
- `obfuscate`, `public_comment`, and `private_comment` shape the listings. `GET /api/v1/instance/domain_blocks` lists `silence` and `suspend` blocks publicly with each domain's SHA-256 digest, its severity, and its public comment. An obfuscated block shows the domain with its middle characters replaced by `*`, as Mastodon's `public_domain` does.

Each isolate caches the block set for 60 seconds, so another isolate applies an admin edit within that time.

### IP Blocks

Admins manage IP blocks through Mastodon's `/api/v1/admin/ip_blocks` (scopes `admin:read:ip_blocks` and `admin:write:ip_blocks`). The admin UI has no IP block page. A block names an IPv4 or IPv6 CIDR range, stored in canonical form (`10.1.2.3/8` is stored as `10.0.0.0/8`), an optional `expires_in` in seconds, a comment, and a severity:

- `no_access` answers every request from the range with `403` before routing, except `GET /healthz`.
- `sign_up_block` refuses `POST /api/v1/accounts` from the range with `403`.
- `sign_up_requires_approval` creates the account but holds it for an admin to approve, as described in [Registration Approval](#registration-approval).

When several blocks cover an address, the strictest one in force applies. The client address is the `CF-Connecting-IP` header, which Cloudflare sets on every request it forwards to the Worker. An IPv4-mapped IPv6 address matches IPv4 ranges. A request without the header, such as a local `wrangler dev` call, matches no block. `X-Forwarded-For` is never read. Each isolate caches the block list for 60 seconds, and a failed D1 read applies no block rather than refusing every client.

### Registration Approval
<!-- derived-from #ip-blocks -->
<!-- constrained-by ../reference/model-refinement.md#catalog -->

`POST /api/v1/accounts` holds a new account for review when `REGISTRATIONS_MODE` is `approved` (Mastodon's `registrations_mode = approved`) or the client address is in a `sign_up_requires_approval` range. `cfwdon_domain::sign_up_approval` decides it. A `sign_up_block` range is refused before the form is read. A held account gets a row in `pending_account_approvals`, which the account insert writes in the same D1 batch, so the account never exists as approved first. The row keeps the sign-up `reason`. The sign-up still answers with an access token, as Mastodon does.

A pending account cannot use the API. Its OAuth token decodes as no account, and an Auth0 sign-in with its e-mail resolves to no account. An API request that would answer `401` then answers `403` with `{"error":"Your login is currently pending approval"}`, Mastodon's `require_user!` message. The Auth0 OAuth authorize page answers the same `403` instead of "no local account". Endpoints that also serve anonymous readers treat the request as anonymous. Accounts that the Auth0 path provisions itself are never held, as Mastodon approves OmniAuth (`external`) sign-ups.

Admins review held accounts with Mastodon's `POST /api/v1/admin/accounts/:id/approve` and `POST /api/v1/admin/accounts/:id/reject` (scope `admin:write:accounts`), or from the admin UI's "承認待ちアカウント" page, which lists `GET /api/cfwdon/admin/accounts/pending`. Approval deletes the pending row and answers the `Admin::Account` document. Rejection deletes the account, and its token, password, keys, and e-mail confirmation go with it through their foreign keys, so the username and e-mail can sign up again. It answers `{}`. Both answer `403` for an account that is not pending and `404` for an unknown id, and both write conditionally on the pending row, so two concurrent reviews cannot both act. The `registration_pipeline` Stateright model checks that a pending account never has API access and that only an open sign-up or an admin's approval grants it.

`approval_required` in `/api/v1/instance` and `/api/v2/instance` reports `REGISTRATIONS_MODE`.

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
- Two cron triggers run maintenance: hourly (outbox queue kick, expired polls, scheduled statuses, background jobs such as card unfurls, stale inbox reclaim, remote content retention, federation activity retention, the stored media metadata backfill, cache refresh) and every six hours (trending tags and statuses). Orphaned media is pruned through `POST /internal/media/prune-orphans`, not cron.

## Observability

`[observability]` logs and traces are enabled in `wrangler.toml.example`. The project prefers structured JSON logs with request IDs, actor IDs, delivery targets, retry counts, and route context. Important event classes include D1 failures, R2 failures, remote fetch failures, signature verification failures, delivery retries, terminal delivery failures, and inbox replay decisions (`inbox_replay_skipped`, `inbox_activity_in_flight`).

## Reliability And Failure Modes

- Missing D1/R2 bindings should fail with clear configuration errors.
- Media writes need recovery paths for partial R2/D1 success.
- Outbound delivery must remain idempotent across retries.
- Inbox processing must dedupe replayed activity IDs, and must not answer `2xx` to a duplicate whose first apply has not finished (see [Inbox Claims](#inbox-claims)).
- Remote fetches must keep SSRF defenses active for actor, inbox, public key, and status URLs.
- Private visibility checks must stay centralized enough that API and ActivityPub reads agree.

## Security And Privacy

- Store API keys and private material as Cloudflare secrets or D1 data as appropriate; do not commit secrets.
- Keep public media outside the protected API surface.
- Validate Auth0 JWT issuer and audience fail-closed.
- Validate incoming ActivityPub signatures, dates, and digests fail-closed for signed inbox traffic.
- Maintain DNS-based SSRF checks for remote resolution paths.
- Treat private/direct status visibility as a cross-cutting data access concern.
- Strip location and device metadata from user media before it is stored, as described in [Media Metadata Stripping](#media-metadata-stripping).

### Media Metadata Stripping

Cameras and phones write the GPS position, device, owner, and capture time into photos and videos. Mastodon removes them by re-encoding with libvips and ffmpeg. A Worker has neither, so `cfwdon_domain::media_metadata` rewrites the container losslessly. It decodes no pixels or samples, and its time is linear in the file size. The in-place overwrites are kept as merged byte ranges, so a file that points many metadata ranges at the same bytes writes each byte once. An `iloc` that lists an item twice, and a sample table that claims more metadata bytes than `mdat` holds, are refused as malformed.

| Container | Removed | Kept |
| --- | --- | --- |
| JPEG | APP1 EXIF and XMP, APP13 IPTC, other APPn, comments, bytes after EOI | JFIF, ICC profile, Adobe marker, image data, a non-default EXIF orientation |
| PNG | `tEXt`, `zTXt`, `iTXt`, `tIME`, `eXIf`, private chunks | critical, colour, and animation chunks, a non-default orientation |
| WebP | `EXIF`, `XMP `, and unknown chunks, also inside `ANMF` frames; the VP8X flags and RIFF size are corrected | `ICCP`, `ALPH`, `ANIM`, `ANMF` frame headers, image data, a non-default orientation |
| HEIF (AVIF, HEIC) | EXIF and XMP items, overwritten in place; an image sequence's `moov` is stripped as an MP4's is | everything else, including `irot` and `imir` |
| MP4, MOV | `udta`, `meta`, `XMP_`, `prft`, metadata `uuid` boxes, and timed metadata tracks such as GoPro GPS, timed text tracks (`text`, `sbtl`, `subt`) and timecode tracks (`tmcd`), replaced by `free` boxes of the same size with their samples zeroed; `mvhd`, `tkhd`, and `mdhd` times are zeroed | sample data and every offset |
| WebM, Matroska | `Tags`, `Attachments`, title, date, file names, segment UIDs, and track names, replaced by `Void` elements of the same size | clusters, cues, chapters, muxing and writing app |

The orientation goes back in as a new EXIF block that holds only that tag, so a rotated phone photo still displays upright.

Timed text goes even when it holds captions a person wrote. DJI drones write their GPS position, height, and camera settings as a caption on every frame. Chrome and Firefox do not show an MP4's own text track, so few viewers lose captions they would have seen.

Every path that stores user media calls `strip_upload_metadata` before the R2 put: media attachments, avatars and headers, and custom emoji. The bytes decide the container, not the declared type. The upload is refused with 422 when the file is damaged, when a declared image or video type does not match the bytes, or when the file is TIFF, which is EXIF throughout. GIF, audio, and other formats without a stripper are stored as uploaded.

Objects stored before stripping existed are queued once in `media_metadata_backfill` by migration `127`. The hourly cron runs the backfill after all other hourly maintenance. Each run reads up to 200 queued objects, or 64 MiB, rewrites each with a conditional put on the object's etag, then deletes its row. An object is read and written back in chunks, so it costs at most about twice its size in memory, and every object up to the 40 MiB upload limit is stripped. A run counts an attempt before it reads an object. An object that ends the invocation sorts behind the rest, and after 3 attempts it is dequeued and logged as `abandoned`. Objects over 40 MiB, TIFF files, and files that cannot be parsed stay as they are and are logged as `media_metadata_backfill` events with the outcome `too_large`, `unstrippable`, or `unreadable`. Copies already cached by the media domain or by browsers keep their old bytes until they expire or are purged; see [Stripping Metadata From Stored Media](../operations/cloudflare-deploy.md#stripping-metadata-from-stored-media).

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
