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
- `auth` handles Auth0 JWT verification, credential lookup with the account's approval, and authenticated account lookup. It never creates accounts; `POST /api/v1/accounts` does.
- `request_utils.rs`, `response_utils.rs`, `time_html.rs`, `db_utils.rs`, `id_utils.rs`, and `content_helpers.rs` provide shared request, response, time, query, ID, and content helpers.
- `identity.rs` centralizes instance domain, actor URL, WebFinger, shared inbox, remote ID, and authority normalization.
- `db_session.rs` opens a request-scoped D1 Sessions API session: `GET`/`HEAD` start `first-unconstrained` so reads can use read replicas, mutating methods start `first-primary`, and an `x-d1-bookmark` request header continues an earlier session. It is wired into selected read-heavy routes (timelines, notifications, status detail). Session handles keep the primary binding, so a timeline `max_id`/`min_id`/`since_id` lookup that misses on a lagging replica retries on the primary (streaming can deliver a status before replicas have it) and logs `timeline_cursor_replica_miss`; see [D1 Sessions API Spike](../planning/d1-sessions-api-spike.md).
- `stream_hub/` is the `StreamHub` Durable Object (bound as `STREAM_HUB`) behind WebSocket and SSE streaming; `stream_hub_publish.rs` publishes events inline after D1 commits.
  Each hub records its sockets' live subscriptions in the D1 `stream_subscriptions` table. Publishers read that table at most once per 5 seconds per isolate and skip, before any payload rendering, every hub with no matching subscriber; so an event that comes within 5 seconds of a new subscription can be missed. A hub that delivers a publish to nobody rewrites its rows from its live sockets, which clears rows left by sockets that died without a close.
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
- `severity: "silence"` limits the server's accounts as Mastodon's `silence` does. Federation still works. See [Silenced Accounts](#silenced-accounts).
- `severity: "noop"` limits nothing on its own and carries only the flags below.
- `reject_media` stores no attachments for the server's remote posts and no avatar or header for its accounts. The Worker never downloads remote media, so this keeps the server's media URLs out of D1 and out of API responses. Rows stored before the block keep their media until the post or profile is refreshed.
- `reject_reports` drops every inbound `Flag` from the server, so it files no report (see [Inbound Reports](#inbound-reports)).
- `obfuscate`, `public_comment`, and `private_comment` shape the listings. `GET /api/v1/instance/domain_blocks` lists `silence` and `suspend` blocks publicly with each domain's SHA-256 digest, its severity, and its public comment. An obfuscated block shows the domain with its middle characters replaced by `*`, as Mastodon's `public_domain` does.

Each isolate caches the block set for 60 seconds, so another isolate applies an admin edit within that time.

#### Silenced Accounts
<!-- derived-from #instance-domain-blocks -->

A remote account is silenced when a `silence` or `suspend` block covers its acct domain or its actor URI's host. cfwdon has no per-account limit (Mastodon's `Account#silence!`), so domain blocks are the only source. Every check below goes through `InstanceDomainBlocks::restrictions_for_actor`, and the viewer-dependent ones through `store::relationship::silenced_authors_hidden_from`, which reads the viewer's accepted follows only when one of the authors is silenced. The notification list looks up its senders' acct domains only when a `silence` block exists or a `suspend` block covers a sender's actor URI host, so an instance whose only blocks are suspensions pays no extra query. As in Mastodon:

- Its posts stay off the public and hashtag timelines and their streams, even for followers (`PublicFeed`, `FanOutOnWriteService#broadcastable?`). They never trend, and its accounts are left out of `GET /api/v1/directory` (`Account.discoverable`).
- A local account that follows it still gets its posts in the home timeline, lists, and notifications.
- For anyone else, including anonymous readers, its replies are left out of a thread's ancestors and descendants (`StatusFilter#silenced_account?`), and its posts are left out of status search results (`StatusesSearchService`). Opening one of its posts directly, or looking it up by URL, still works.
- A notification from it to a local account that does not follow it follows that account's `for_limited_accounts` policy for the kinds Mastodon lets a policy filter (mention, reblog, favourite, follow, follow request, quote). `accept` keeps it. `filter`, the default, and `drop` leave it out of the notification list, the unread count, the streaming API, and Web Push (only a remote quote pushes a filterable kind). Mastodon files a filtered notification under notification requests. cfwdon's notification requests hold only follow requests, so a filtered notification is not listed there. Other kinds, such as poll results and edits, are kept.
- cfwdon stores no notification rows; it derives them from the activity tables when they are read. So the rule is applied when a notification is listed, counted, streamed, or pushed, not once at creation as Mastodon's `NotifyService` does. A change to `for_limited_accounts`, a new follow, or a lifted block also applies to notifications that already exist. A push or stream event already sent stays sent.
- A follow from it waits for approval, as if the local account were locked.

Account search (`AccountSearchService`) does not filter silenced accounts in Mastodon, so cfwdon does not either. Follow suggestions list only local accounts.

### Inbound Reports
<!-- derived-from #instance-domain-blocks -->

Another server files a report here by delivering an ActivityPub `Flag` to a personal inbox or the shared inbox. cfwdon handles it the way Mastodon's `ActivityPub::Activity::Flag` does:

- A `Flag` from a server whose domain block has `reject_reports` (or `severity: "suspend"`) files nothing. The block is matched on the sender's acct domain and on its actor URI host.
- The `Flag` files one report for each local account its `object` names. A report carries only the named local statuses that the account wrote. A status alone does not make its author a target, and remote or unknown objects are ignored. At most 100 objects are looked up.
- The reporter is the actor that signed the `Flag`. For an anonymous report this is the remote server's instance actor. The report's category is `other`, its comment is the `Flag` `content` cut to 1,000 characters, and `origin_domain` records the sender's domain. The admin reports page shows that domain as the report's source.
- A report is keyed on the `Flag` id and the target account, so a redelivered `Flag` files nothing new. The id counts only when its host matches the sender's actor URI. A `Flag` with any other id is not keyed and relies on the inbox claim alone.
- Each new report sends the `admin.report` push to the configured admins. It also lists in their `admin.report` notifications with the remote reporter as the notification's account.

### IP Blocks

Admins manage IP blocks through Mastodon's `/api/v1/admin/ip_blocks` (scopes `admin:read:ip_blocks` and `admin:write:ip_blocks`). The admin UI has an IP blocks page (`/admin/ip-blocks`) backed by `/api/cfwdon/admin/ip-blocks` (list, create, `PATCH`/delete by id), which share the Mastodon handlers and so the same administrator check. A block names an IPv4 or IPv6 CIDR range, stored in canonical form (`10.1.2.3/8` is stored as `10.0.0.0/8`), an optional `expires_in` in seconds, a comment, and a severity:

- `no_access` answers every request from the range with `403` before routing, except `GET /healthz`.
- `sign_up_block` refuses `POST /api/v1/accounts` from the range with `403`.
- `sign_up_requires_approval` creates the account but holds it for an admin to approve, as described in [Registration Approval](#registration-approval).

When several blocks cover an address, the strictest one in force applies. The client address is the `CF-Connecting-IP` header, which Cloudflare sets on every request it forwards to the Worker. An IPv4-mapped IPv6 address matches IPv4 ranges. A request without the header, such as a local `wrangler dev` call, matches no block. `X-Forwarded-For` is never read. Each isolate caches the block list for 60 seconds, and a failed D1 read applies no block rather than refusing every client.

### Registration Approval
<!-- derived-from #ip-blocks -->
<!-- constrained-by ../reference/model-refinement.md#catalog -->

`POST /api/v1/accounts` holds a new account for review when `REGISTRATIONS_MODE` is `approved` (Mastodon's `registrations_mode = approved`), which is also what an unset, blank, or unrecognized value means, or the client address is in a `sign_up_requires_approval` range. `cfwdon_domain::sign_up_approval` decides it. A `sign_up_block` range is refused before the form is read. A held account gets a row in `pending_account_approvals`, which the account insert writes in the same D1 batch, so the account never exists as approved first. The row keeps the sign-up `reason`. The sign-up still answers with an access token, as Mastodon does.

A pending account cannot use the API. Every credential lookup returns the account with its approval: the OAuth token query and the Auth0 e-mail query both join `pending_account_approvals`. An endpoint that needs an account answers `403` with `{"error":"Your login is currently pending approval"}`, Mastodon's `require_user!` message. Handlers that only see "no account" answer `401`, and the router turns that into the `403` because the lookup recorded the refusal in the request's own auth state (`auth::request_state`), not in isolate-wide state that a concurrent request could read. Endpoints that also serve anonymous readers treat the request as anonymous, as Mastodon calls `require_user!` only on actions that need a user. Public timelines, collection reads, and search answer a pending account as they answer no credential; where that answer is a `401` because the action needs a user (search with `resolve` or `offset`), the router turns it into the pending `403`. Mastodon would run those searches as the pending user; cfwdon refuses them, so a pending account cannot make the server fetch remote resources. Collection writes refuse a pending account with the `403`. A streaming token in `access_token` or `Sec-WebSocket-Protocol` is refused like one in the header. The e-mail confirmation endpoints (`POST /api/v1/emails/confirmations` and `GET /api/v1/emails/check_confirmation`) accept a pending account, as Mastodon skips `require_user!` there. A pending account can still obtain tokens (sign-up, password login, authorization code exchange), as in Mastodon; they start working when an admin approves it. The Auth0 OAuth authorize page answers the `403` instead of "no local account".

An Auth0 or Access identity only signs in to an account that `POST /api/v1/accounts` created. It never creates one, on any path, so `REGISTRATIONS_MODE` and IP sign-up blocks govern every account, and a rejected identity cannot come back as an approved account. Mastodon approves OmniAuth (`external`) sign-ups; cfwdon has no such sign-up. `ADMIN_EMAILS` does not approve a sign-up either: the sign-up e-mail is not verified, so anyone could sign up with an admin's address. The first admin is approved by the operator with `just approve-account`, which deletes the `pending_account_approvals` row through `wrangler d1 execute` (see [The First Admin](../operations/launch-checklist.md#the-first-admin)).

Elsewhere a pending account does not exist, as with Mastodon's `check_account_approval`. `find_account_by_id` and `find_account_by_username` leave pending accounts out, so the REST account, lookup, statuses, and relationships routes, the ActivityPub actor, WebFinger, and inbox targets answer as for an unknown account. Account search and status `mentions[]` leave them out, so a mention of a pending account stays plain text. Only the admin review, sign-up uniqueness, password sign-in, and token issuance ask for `AccountLookup::IncludingPending`. `find_account_where` keys on the `AccountKey` enum, which has no e-mail column, so an e-mail resolves to an account only through `find_credential_account_by_email`. `scripts/check_pending_account_guard.py` (run by `just ci-server`, with `--self-test` against the forms that slipped past earlier versions) fails on SQL that names both `oauth_access_tokens` and `accounts`, compares `access_email` in any form, or reads `accounts` by a runtime column, unless the statement joins or subqueries `pending_account_approvals`; on `AccountLookup::IncludingPending` outside its allowlist of functions; and on a `find_account_where` call outside `auth/account_store.rs`.

Admins review held accounts with Mastodon's `POST /api/v1/admin/accounts/:id/approve` and `POST /api/v1/admin/accounts/:id/reject` (scope `admin:write:accounts`), or from the admin UI's "承認待ちアカウント" page, which lists `GET /api/cfwdon/admin/accounts/pending`. Approval deletes the pending row and answers the `Admin::Account` document. Rejection deletes the account, and its token, password, keys, and e-mail confirmation go with it through their foreign keys, so the username and e-mail can sign up again. It also drops the cached account and actor documents in the colo that served it; other colos let them expire. It answers `{}`. Both answer `403` for an account that is not pending and `404` for an unknown id, and both write conditionally on the pending row, so two concurrent reviews cannot both act. The `registration_pipeline` Stateright model checks that a pending account never has API access and that only an explicit `open` setting or an admin's approval grants it.

`REGISTRATIONS_MODE=none` refuses every sign-up with `403`, as Mastodon's `registrations_mode = none` does. `registrations` in `/api/v1/instance`, `registrations.enabled` in `/api/v2/instance`, and `openRegistrations` in NodeInfo are `false` only in that mode; `approval_required` is `true` only in `approved` mode. A sign-up `reason` over 420 characters (Mastodon's `UserInviteRequest::TEXT_SIZE_LIMIT`) fails validation with `422` under `details.reason`.

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
- Accept only image, video, and audio types that cannot run script, and serve stored media so it never renders as a page, as described in [Media Content Types](#media-content-types).

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

### Media Content Types

User-supplied bytes reach R2 through media attachments, avatars and headers, and custom emoji. Remote media is neither cached nor proxied: the API hands clients the remote URL.

Uploads are checked against one allowlist, `UPLOAD_MEDIA_TYPES` in `store::media`. It holds Mastodon's `MediaAttachment` image, video, and audio types, without `video/x-ms-asf`, plus `image/jpg`. `GET /api/v2/instance` advertises the same list as `supported_mime_types`. For a media attachment, a declared type outside the list is replaced by the type the magic bytes show. An avatar or header keeps its declared type. When no accepted type remains, the upload is refused with 422. Custom emoji keep their narrower PNG, GIF, and WebP list.

- SVG is refused. An SVG is an XML document that runs script when a browser opens it, and the Worker has no rasterizer to convert it.
- HTML, XML, and any other markup type are refused for the same reason.
- JPEG XL is refused. `cfwdon_domain::media_metadata` has no JPEG XL stripper, so its EXIF and XMP boxes would be stored as uploaded. Mastodon does not accept it either.

The `/media/:id` route serves stored bytes with `X-Content-Type-Options: nosniff` and `Content-Security-Policy: default-src 'none'; form-action 'none'`, as Mastodon's media domain does. An object whose stored type is on the allowlist keeps that type and `Content-Disposition: inline`. Any other stored type, such as an SVG uploaded before the allowlist existed, is served as `application/octet-stream` with `Content-Disposition: attachment`, so it downloads and never renders. Stored SVG objects are not deleted or converted. Clients show them as broken media.

When `MEDIA_PUBLIC_BASE_URL` is set, `/media/:id` redirects there, and R2 serves objects with their stored metadata. The Worker cannot set headers on that domain; see [Serving Media Safely](../operations/cloudflare-deploy.md#serving-media-safely).

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
