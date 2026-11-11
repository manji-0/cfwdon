# Configuration Reference

`cfwdon` runs as a single Cloudflare Worker. It receives relational state through a D1 binding, media storage through an R2 binding, KV namespaces for short-lived caches, a Durable Object binding for streaming, a Queue binding for outbound delivery, Web and admin UI files through a Workers static assets binding, and most runtime settings through `wrangler.toml` `[vars]` plus Cloudflare secrets.

For a safe starting point, copy [`wrangler.toml.example`](../../wrangler.toml.example) to `wrangler.toml` and replace the placeholder values before deploying.

## Cloudflare Bindings
<!-- derived-from ../../wrangler.toml.example -->
<!-- dagayn: implemented-by crates/cfwdon-worker/src/ui_assets.rs::serve_ui_asset -->
<!-- dagayn: implemented-by crates/cfwdon-worker/src/public_endpoint_cache.rs::load_public_endpoint_cache -->

| Binding | Type | Required | Notes |
| --- | --- | --- | --- |
| `DB` | D1 database | Yes | Database binding expected by the Worker runtime and by the defaults in `crates/cfwdon-core/src/config.rs`. |
| `MEDIA` | R2 bucket | Yes | Bucket binding used for uploaded media bodies and profile media objects. |
| `REMOTE_DNS_CACHE` | KV namespace | Yes | Caches remote hostname DoH SSRF validation results for ActivityPub fetches. |
| `APP_CACHE` | KV namespace | Yes | Caches short-lived app data: account capability bits (skip empty D1 probes), public endpoint payloads (`public_timeline`, `instance_activity`), and trend lists. |
| `STREAM_HUB` | Durable Object | Yes | `StreamHub` binding for Mastodon streaming fan-out. |
| `RATE_LIMIT_COUNTER` | Durable Object | Recommended | `RateLimitCounter` binding holding the [rate limit](#rate-limits) counters. Without it nothing is counted. The binding name is a constant in `crates/cfwdon-worker/src/rate_limit.rs`. |
| `OUTBOX_PROCESS_QUEUE` | Queue producer and consumer | Yes | Queue `cfwdon-outbox-process` drains pending outbound ActivityPub deliveries. The Worker enqueues a kick after successful write requests, from `POST /internal/outbox/process`, and from the hourly cron when work is pending. The binding name is a constant in `crates/cfwdon-worker/src/delivery.rs`, not an `AppConfig` field. |
| `ASSETS` | Workers static assets | Yes | Staged `web-ui` and `admin-ui` files under `/app` and `/admin`. Configured by `[assets]` in `wrangler.toml`, not by `AppConfig`. |

If a D1, R2, KV, or Durable Object binding name changes, keep `wrangler.toml`, `cfwdon_core::AppConfig` defaults, and Worker runtime code in sync. The `StreamHub` and `RateLimitCounter` classes need the `[[migrations]]` entries (`v1`, `v2`) shown in `wrangler.toml.example`. The static assets binding name is `ASSETS` in `wrangler.toml` and `crates/cfwdon-worker/src/ui_assets.rs`.

Do not set `[assets] not_found_handling` to `single-page-application`. Unmatched `/app` routes fall through to the Worker so ActivityPub and Mastodon API paths are not captured as HTML.

## Worker And D1 Placement
<!-- derived-from ../../wrangler.toml.example -->

Mastodon API and inbox handlers issue several sequential D1 queries per request. Those round trips stay short only when the Worker runs next to the D1 primary. `wrangler d1 info` only reports a coarse region (`APAC`). The city is in each remote query's `meta.served_by_colo` (airport code). This instance's primary is `NRT` (Tokyo); it was `SIN`, then `KIX`, before the moves described in [Moving The Database](../operations/cloudflare-deploy.md#moving-the-database).

Cloudflare has no `cf:sin` placement key, so pin the Worker to the AWS region whose nearest Cloudflare colo matches that colo. Pinning to Tokyo while the primary is Singapore leaves write latency unchanged: each sequential write still crosses Tokyo–Singapore.

| D1 primary (`served_by_colo`) | Worker `[placement] region` | Typical audience |
| --- | --- | --- |
| `SIN` (usual `--location=apac`) | `aws:ap-southeast-1` | Writes next to APAC primary |
| `NRT` (this instance) | `aws:ap-northeast-1` | Japan (Tokyo), only if the primary actually landed there |
| `KIX` (usual when created without a hint from Japan) | `aws:ap-northeast-3` | Japan (Osaka) |
| `SYD` (`--location=oc`) | `aws:ap-southeast-2` | Australia |
| `LHR` (`--location=weur`) | `aws:eu-west-1` | Western Europe |
| `FRA` (`--location=eeur`) | `aws:eu-central-1` | Central / Eastern Europe |
| `EWR` / `IAD` (`--location=enam`) | `aws:us-east-1` | Eastern North America |
| `SFO` / `LAX` (`--location=wnam`) | `aws:us-west-2` | Western North America |

Check the live primary:

```sh
wrangler d1 execute cfwdon --remote --command="SELECT 1" --json
```

Look for `"served_by_primary": true` and `"served_by_colo"` matching the placement row (`"NRT"` here). D1 location is immutable after create.

`[placement] mode = "smart"` and `region = "..."` are mutually exclusive. Prefer the explicit region: Smart Placement needs multi-location traffic, takes up to 15 minutes to decide, and can follow outbound ActivityPub fetches instead of D1.

After deploy, responses include `cf-placement` (for example `remote-SIN`) when placement routed the `fetch` handler. Static assets still serve from the colo nearest the client. Cron and queue consumers are not placed by this setting.

## Public Instance Vars

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `INSTANCE_DOMAIN` | Recommended | `example.com` | Instance domain used in WebFinger, actor URIs, and API payloads. |
| `INSTANCE_NAME` | Recommended | `cfwdon` | Public instance name. |
| `INSTANCE_DESCRIPTION` | Recommended | Cloudflare Workers based description | Public instance description. |
| `SOURCE_URL` | Optional | unset | Source repository URL advertised by instance endpoints. |
| `INSTANCE_LANGUAGES` | Optional | app default | Comma-separated language list. |
| `CONTACT_EMAIL` | Optional | unset | Contact metadata for instance payloads. |
| `INSTANCE_THUMBNAIL_URL` | Optional | unset | Public thumbnail URL. |
| `ADMIN_EMAILS` | Optional | empty | Comma-separated admin e-mail list for admin notifications. |
| `MEDIA_PUBLIC_BASE_URL` | Required for production media URLs | unset | R2 custom domain base URL. A trailing slash is trimmed. |

Keep the `MEDIA_PUBLIC_BASE_URL` domain publicly reachable. Mastodon entity payloads use this value as the canonical media URL.

## Auth0 Authentication Vars
<!-- derived-from ../../wrangler.toml.example -->
<!-- constrained-by ../operations/auth0-configuration.md#web-ui-session-lifetime -->

Protected API routes validate Auth0-issued RS256 JWTs. By default, the Worker reads a Bearer token from `Authorization`, validates `iss` against `AUTH0_DOMAIN`, validates `aud` against `AUTH0_AUDIENCE`, fetches signing keys from `/.well-known/jwks.json`, and maps `AUTH0_EMAIL_CLAIM` to a local account e-mail. Browser login redirects use Auth0 Authorization Code with PKCE, return through `/oauth/auth0/callback`, set HttpOnly access and refresh cookies, and then continue the Mastodon OAuth consent flow. The Web UI refresh cookie lasts 7 days; see [Auth0 Configuration Guide](../operations/auth0-configuration.md#web-ui-session-lifetime).

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `AUTH0_JWT_HEADER` | Optional | `Authorization` | Header containing the Auth0 JWT. When this is `Authorization`, the Worker accepts standard `Bearer <token>` values. |
| `AUTH0_DOMAIN` | Required for protected API | unset | Auth0 tenant domain or issuer URL, for example `example.us.auth0.com`. |
| `AUTH0_CLIENT_ID` | Required for browser login redirects | unset | Auth0 application client ID used when redirecting `/oauth/authorize` flows to Auth0. |
| `AUTH0_AUDIENCE` | Required for protected API | unset | Auth0 API identifier expected in the JWT `aud` claim. |
| `AUTH0_EMAIL_CLAIM` | Required for protected API | `email` | String claim used to map the Auth0 user to `accounts.access_email`. |
| `AUTH0_ROLES_CLAIM` | Optional | `{AUTH0_AUDIENCE}/roles` | JWT claim containing Auth0 role names. Used with `AUTH0_ADMIN_ROLES` for `/admin` access. |
| `AUTH0_ADMIN_ROLES` | Optional | empty | Comma-separated Auth0 role names that grant `/admin` access. `ADMIN_EMAILS` remains a fallback when set. |

Set real values for `AUTH0_DOMAIN`, `AUTH0_CLIENT_ID`, and `AUTH0_AUDIENCE`. Do not deploy production with placeholders. Auth0 domain, client ID, audience, and claim names are configuration values rather than secrets, so keep the canonical deployment values in `wrangler.toml`.

In the Auth0 application settings, include:

- Allowed Callback URLs: `https://<INSTANCE_DOMAIN>/oauth/auth0/callback`
- Allowed Logout URLs: `https://<INSTANCE_DOMAIN>`
- Allowed Web Origins: `https://<INSTANCE_DOMAIN>`
- Allowed Origins (CORS): `https://<INSTANCE_DOMAIN>`

For the complete Auth0 Dashboard setup, API audience selection, PKCE application requirements, and e-mail claim mapping, see [Auth0 Configuration Guide](../operations/auth0-configuration.md).

Cloudflare Access can protect private or operational Worker hostnames as an edge access gate, but it should not sit in front of the normal public Auth0 callback path. The current Worker does not treat Access JWTs as local user authentication. For the supported deployment boundary and policy setup, see [Cloudflare Access Configuration Guide](../operations/cloudflare-access-configuration.md).

## Policy And Instance Content Vars
<!-- constrained-by #public-instance-vars -->

Short instance blurb goes in `INSTANCE_DESCRIPTION`. Longer about / privacy / terms bodies can be set as HTML or plain text. Plain text is wrapped in paragraphs automatically; when both plain and HTML vars are set, the HTML var wins.

| Var | Required | Notes |
| --- | --- | --- |
| `INSTANCE_EXTENDED_DESCRIPTION` | Optional | Plain-text about / extended description body. |
| `INSTANCE_EXTENDED_DESCRIPTION_HTML` | Optional | HTML about / extended description body. |
| `INSTANCE_EXTENDED_DESCRIPTION_UPDATED_AT` | Optional | Extended description timestamp (`updated_at`). |
| `PRIVACY_POLICY` | Optional | Plain-text privacy policy body. |
| `PRIVACY_POLICY_HTML` | Optional | HTML privacy policy body. |
| `PRIVACY_POLICY_UPDATED_AT` | Optional | Privacy policy timestamp (`updated_at`). |
| `TERMS_OF_SERVICE` | Optional | Plain-text terms of service body. |
| `TERMS_OF_SERVICE_HTML` | Optional | HTML terms of service body. |
| `TERMS_OF_SERVICE_EFFECTIVE_DATE` | Optional | Terms effective date (`YYYY-MM-DD`). |

When these bodies are unset, the Worker falls back to `INSTANCE_DESCRIPTION` for extended description, privacy policy, and terms of service so the Mastodon `configuration.urls` links stay resolvable. Keep Cloudflare var size limits in mind; move large or frequently edited content behind a storage-backed path later instead of expanding vars indefinitely.

## Timeline Access Vars

Each value is one of `public`, `authenticated`, or `disabled`. Missing or invalid values fall back to application defaults.

| Var | Scope |
| --- | --- |
| `TIMELINES_ACCESS_LIVE_FEEDS_LOCAL` | Local live/public feed access. |
| `TIMELINES_ACCESS_LIVE_FEEDS_REMOTE` | Remote live/public feed access. |
| `TIMELINES_ACCESS_HASHTAG_FEEDS_LOCAL` | Local hashtag feed access. |
| `TIMELINES_ACCESS_HASHTAG_FEEDS_REMOTE` | Remote hashtag feed access. |
| `TIMELINES_ACCESS_TRENDING_LINK_FEEDS_LOCAL` | Local link timeline access. |
| `TIMELINES_ACCESS_TRENDING_LINK_FEEDS_REMOTE` | Remote link timeline access. |

## StreamHub Vars

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `STREAM_HUB_BINDING` | Optional | `STREAM_HUB` | Binding name the `StreamHub` Durable Object uses for hub-to-hub forwarding. Set it only when the binding in `wrangler.toml` is renamed. |

## Federation Retention Vars

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `FEDERATION_ACTIVITY_RETENTION_DAYS` | Optional | `30` | Days to keep terminal federation bookkeeping rows: `delivered` or `failed` rows in `outbox_deliveries` and `outbound_activities`, `expanded` generic rows in `outbox_deliveries` (their per-inbox rows retry on their own), and processed `inbox_activities`. The hourly cron deletes at most 5,000 older rows per table per run, in batches of 500. Queued and `in_flight` rows are never deleted. Set `0` to keep every row. A blank or non-numeric value keeps the default. |

## Authorized Fetch (Secure Mode)
<!-- derived-from ../../wrangler.toml.example -->
<!-- constrained-by ./model-refinement.md#catalog -->

`AUTHORIZED_FETCH` turns on Mastodon's secure mode. It is off unless set to `true`, `1`, `yes`, or `on`. With it on, a GET of this server's own ActivityPub resources needs a valid HTTP signature (draft-cavage) from a remote actor on a domain that is not suspended. `cfwdon_domain::authorized_fetch_access` decides each GET, and the `authorized_fetch` Stateright model checks the decision (see [Model Refinement Mapping](./model-refinement.md#catalog)).

| Path | Unsigned, or the signature does not verify | Signed by an allowed actor | Signer's key or actor on a suspended domain |
| --- | --- | --- | --- |
| `/users/:username`, `/@:username` (an actor) | `200` with the restricted document | `200`, the whole document | `403` |
| `/users/:username/statuses/:id`, `/@:username/:id` (a note) | `401` | `200`, or `404` when the author blocks the signer | `403` |
| `/users/:username/statuses/:id/quote_authorizations/:key` | `401` | `200`, or `404` when the author blocks the signer | `403` |
| `/users/:username/outbox` | `401` | `200`; pages are empty when the owner blocks the signer | `403` |
| `/users/:username/collections/featured`, `/collections/tags` | `401` | `200`; empty when the owner blocks the signer | `403` |
| `/users/:username/followers`, `/following` | `401` | `200` | `403` |

**Signatures.** The signature must cover `(request-target)`, `host`, and either `date` or the hs2019 `(created)`, as Mastodon's `verify_signature_strength!` requires. A GET has no body, so no `Digest` is needed. A signed `Date` must be at most 12 hours old and at most 1 hour ahead of this server's clock, as for inbox deliveries. A signed `(created)` is checked as Mastodon's `matches_time_window?` checks it: `created` at most 1 hour ahead, and the request at most 1 hour after `expires`, which defaults to 5 minutes after `created` and is capped at 12 hours after it. `(created)` and `(expires)` are accepted only when the `algorithm` is `hs2019` or absent. The key's actor is resolved as for an inbox delivery: from the cache, or fetched again when the cached key does not verify. That refetch happens at most once per actor every 5 minutes (`REMOTE_KEY_REFETCH_COOLDOWN_MS`), counted from the last fetch by any isolate (the `remote_actors.updated_at` the fetch stamps) or a failed fetch by this isolate, so requests that name a foreign `keyId` with a bogus signature cannot make the Worker fetch that actor on every request. The same cooldown applies to inbox deliveries. A sender whose key rotated within the window is refused until the window passes. A key on a suspended domain is refused before anything is fetched from its server. "Blocks the signer" means the local user blocked the signer's account, or blocked its domain (or a parent domain), as Mastodon's `StatusPolicy#show?` and `for_signed_account` do.

**The restricted actor document.** An unsigned GET of an actor gets only `@context`, `id`, `type`, `preferredUsername`, `inbox`, `outbox`, `publicKey`, and `endpoints` (Mastodon's `restrict_fields_to`). A remote server can still read the key it needs to verify that actor's signatures.

**Owner blocks apply to ActivityPub only.** A local user's blocks hide their content only from the ActivityPub representation a signer fetches. The REST API (`/users/:username/statuses`, `/api/v1/accounts/:id/statuses`) and the HTML pages stay public, as in Mastodon without `DISALLOW_UNAUTHENTICATED_API_ACCESS`.

**Always public.** The instance actor (`/actor` and `/actor/outbox`), `/.well-known/webfinger`, `/.well-known/host-meta`, `/.well-known/nodeinfo`, `/nodeinfo/2.0`, `/nodeinfo/2.1`, the inboxes (they verify their own signatures), the REST API, and the HTML pages. A request for `/@user` or a note whose `Accept` asks for `text/html` (and not ActivityPub) gets the HTML page. Remote servers in secure mode fetch `/actor` to verify the signatures this server sends, so `/actor` must answer unsigned GETs.

**Signed outbound fetches.** In secure mode, this server signs every ActivityPub GET it makes as the instance actor (`/actor#main-key`), as Mastodon does. This includes the key fetches that verify inbound signatures. A failed signed GET is not retried unsigned, as Mastodon never retries one. Outside secure mode, the few fetches that sign as the instance actor still retry unsigned when the remote answers an HTTP error status or a body that is not JSON, because some remotes and the web application firewalls in front of them reject signed GETs of public documents. If `AUTHORIZED_FETCH` is on but the D1 binding cannot be read, the Worker logs `secure_mode_fetch_signing_unavailable` at `warn` level and its outbound GETs go out unsigned. A key fetch cannot loop: the remote server verifies this server's signature with `/actor`, which needs no signature.

**Caching.** Every answer of a guarded path in secure mode carries `Cache-Control: private` and `Vary: Signature`, refusals included, so no shared cache gives one requester's answer to the next. The Worker's own Cache API entries hold the whole documents. In secure mode the Worker reads them only after the signature check allows the whole document, and it builds the restricted actor document from them. A signed request for an outbox page skips the cache, because the owner may block that signer; the outbox root, which lists no posts, is shared. Cloudflare's cache does not vary on `Signature`. Do not add a Cache Rule that caches `/users/*` or `/@*` regardless of the origin `Cache-Control` ("Cache Everything", or an Edge TTL that overrides the origin). Public answers cached before you turn secure mode on expire within their `max-age` (60 seconds).

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `AUTHORIZED_FETCH` | Optional | off | `true`, `1`, `yes`, or `on` turns on secure mode as described above. Any other value leaves it off. |

## Registration Vars
<!-- derived-from ../../wrangler.toml.example -->
<!-- constrained-by ../architecture/cfwdon-architecture.md#registration-approval -->

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `REGISTRATIONS_MODE` | Optional | `open` | Mastodon's `registrations_mode`. `open` takes `POST /api/v1/accounts` sign-ups. `approved` holds each one for an admin to approve and sets `approval_required` in the instance documents. `none` refuses every sign-up with `403`, and the instance and NodeInfo documents report registrations as closed. Unset or blank means `open`. Any other value fails closed: the Worker logs an error and applies `approved`. IP blocks with `sign_up_requires_approval` hold sign-ups from their ranges in `open` mode too. |

## Logging Vars

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `CFWDON_API_REQUEST_LOG` | Optional | enabled | Set to `0`, `false`, `off`, or `no` to stop per-request API log lines. |

## Rate Limits
<!-- derived-from ../../wrangler.toml.example -->

The Worker throttles requests as Mastodon's Rack::Attack and its per-account `RateLimiter` do. The policy (which buckets a request counts against, the defaults, the fixed windows) is `cfwdon_domain::rate_limit`. `crates/cfwdon-worker/src/rate_limit.rs` enforces it in `router::handle_fetch`, after the CORS preflight answer and before any route, so a 429 still carries the CORS headers a browser client needs.

**Buckets and defaults.** Every window is fixed and aligned to the epoch, as Rack::Attack's are (a 5-minute window ends on a multiple of 300 seconds).

| Bucket | Counted per | Requests | Default (Mastodon) |
| --- | --- | --- | --- |
| `api_ip` | client address | `/api/*` without a credential that authenticates | 300 / 5 min (`throttle_unauthenticated_api`) |
| `api_token` | OAuth token or session | `/api/*` with a valid credential (an app token counts here only) | 300 / 5 min (`throttle_per_token_api`) |
| `api_account` | account | `/api/*` with any of the account's credentials | 1500 / 5 min (`throttle_authenticated_api`) |
| `media` | account | `POST /api/v1/media`, `POST /api/v2/media` | 30 / 30 min (`throttle_api_media`, `RateLimiter` `media`) |
| `statuses` | account | `POST /api/v1/statuses` (scheduled ones too) | 300 / 3 h (`RateLimiter` `statuses`) |
| `reblogs` | account | `POST /api/v1/statuses/:id/reblog` | 300 / 3 h (`RateLimiter` `reblogs`) |
| `follows` | account | `POST /api/v1/accounts/:id/follow` | 400 / 24 h (`RateLimiter` `follows`) |
| `reports` | account | `POST /api/v1/reports` | 400 / 24 h (`RateLimiter` `reports`) |
| `sign_in` | client address | `GET`/`POST /oauth/authorize`, `GET /oauth/auth0/callback` (where Auth0 sign-ins create accounts) | 25 / 5 min (`throttle_login_attempts/ip`) |
| `app_registration` | client address | `POST /api/v1/apps` | 5 / 10 min (app registrations per address) |
| `oauth_token` | client address | `POST /oauth/token` | 25 / 5 min (no Mastodon throttle; as login attempts) |
| `federation_fetch` | client address | `GET`/`HEAD` of `/.well-known/*`, `/nodeinfo/*`, `/users/*` | 1500 / 5 min (Mastodon has none) |
| `inbox_ip` | client address | `POST /inbox`, `/users/:username/inbox`, before the body is read | 1500 / 5 min (Mastodon has none) |
| `inbox_domain` | sending server | the same deliveries, once their HTTP signature verified | 3000 / 5 min (Mastodon has none) |
| `remote_lookup` | account, or client address when anonymous | `GET /api/v2/search` and `GET /api/v1/accounts/search` with `resolve=true`, and `GET /api/v1/accounts/lookup` with an `acct` on another domain; each makes the Worker fetch a remote actor | 60 / 5 min (Mastodon and tstodon have none; chosen to stay far below the general API limits without hurting search-as-you-type clients) |

A lookup counts against `remote_lookup` in addition to the general API limits. An app token with no account counts per token. An authenticated API request counts against its credential and its account, not its address, as in Mastodon. A credential is an OAuth bearer token, the Auth0 JWT header, or the Auth0 session cookie; one that does not authenticate counts as its address. Staff are not exempt (Mastodon does not exempt them either). The action families count attempts, where Mastodon's `RateLimiter` counts the statuses, boosts, follows and reports actually created. The streaming endpoint (`/api/v1/streaming`), the Web UI, media, the `/internal/*` routes, and cron and queue work are not counted. `inbox_domain` keys on the host of the actor whose key verified, after the signature check, so a forged or unsigned delivery naming another server cannot spend that server's budget; `inbox_ip` in front of it bounds what a flood of forged deliveries costs.

**Responses.** A request over a limit gets `429` with Mastodon's body, `{"error":"Too many requests"}`, `Retry-After` (seconds to the end of the window) and `X-RateLimit-Limit`, `X-RateLimit-Remaining` (`0`) and `X-RateLimit-Reset` (an ISO 8601 time). Every `/api/*` response that was counted carries the same three headers for the counter with the least left, as Mastodon's API does, and CORS exposes them to browser clients. Remote servers retry a 429 inbox delivery later.

**Client address.** Only `CF-Connecting-IP`, which Cloudflare sets on every request it forwards and overwrites when a client sends its own; `X-Forwarded-For` is not read. Without it (local `wrangler dev`, tests) no address-keyed bucket applies. IPv6 addresses count per `/64` (Mastodon's `throttleable_remote_ip`), since one host routinely holds a whole `/64`.

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `RATE_LIMITS_ENABLED` | Optional | enabled | Set to `0`, `false`, `off`, or `no` to turn every limit off. |
| `RATE_LIMITS` | Optional | Mastodon's defaults | JSON object of overrides by bucket name, each `{"limit": n, "period": seconds}` or `false` to lift that bucket, for example `{"api_ip": {"limit": 600, "period": 300}, "inbox_domain": false}`. Buckets not named keep the defaults. A value that is not JSON, names an unknown bucket, or has a malformed rule is ignored as a whole: the defaults apply and the Worker logs a warning. |

**Storage.** Each subject (an address, an account, an app token, a sending server) is one instance of the `RateLimitCounter` Durable Object (`RATE_LIMIT_COUNTER`, migration tag `v2`), so no instance is shared by unrelated clients. It holds that subject's counters in memory and in its storage. A request costs one Durable Object call per subject it counts against (one for most requests) and no D1 write. An authenticated API request also needs to know whose credential it carries: each isolate looks that up once a minute per credential (one D1 read) and remembers it. An alarm at the end of the last window deletes the instance's storage. Each isolate also remembers a counter it saw over its limit until that window ends and refuses without asking the Durable Object, so a flood from one client costs one call per window per isolate. If the Durable Object cannot be reached the request goes through (fail open) and the Worker logs the error.

The Workers Rate Limiting binding was not used: it offers only 10- and 60-second periods, fixes each limit in `wrangler.toml` at deploy time, counts per Cloudflare location, and reports no remaining count for `X-RateLimit-Remaining`. Workers KV is eventually consistent and takes one write per key per second; D1 would be a write per request. For volumetric floods, put a Cloudflare WAF rate-limiting rule in front of the Worker as an outer layer; it drops traffic before it reaches the Worker.

## Push Notification Vars

| Var | Required | Notes |
| --- | --- | --- |
| `WEB_PUSH_VAPID_PUBLIC_KEY` | Required for advertised push support | Public VAPID key returned in instance/app shapes. |
| `WEB_PUSH_VAPID_PRIVATE_KEY` | Required for delivery | Store as a secret. Push delivery fails without it. |
| `WEB_PUSH_VAPID_SUBJECT` | Required for delivery | VAPID subject, usually `mailto:admin@example.com` or an HTTPS URL. |

Private key material belongs in Cloudflare secrets, not in `wrangler.toml`.

## E-mail Confirmation Vars

| Var | Required | Notes |
| --- | --- | --- |
| `RESEND_API_KEY` | Optional | If missing, pending confirmation dispatch is skipped. Store as a secret. |
| `EMAIL_FROM` | Required when using Resend | Sender address used for confirmation e-mails. |

## Translation Vars

| Var | Required | Notes |
| --- | --- | --- |
| `TRANSLATION_PROVIDER` | Optional | Defaults to `libretranslate`; supported values are `libretranslate` and `deepl`. |
| `TRANSLATION_API_URL` | Required for translation provider config | Provider endpoint URL. |
| `TRANSLATION_API_KEY` | Required for DeepL, optional for LibreTranslate | Store as a secret when set. |

If provider config is incomplete, translation integration is disabled.

## JSON Content Vars

| Var | Required | Notes |
| --- | --- | --- |
| `ANNOUNCEMENTS_JSON` | Optional | JSON source for announcement documents. |
| `DONATION_CAMPAIGN_JSON` | Optional | JSON source for donation campaign documents. |
| `CUSTOM_EMOJIS_JSON` | Optional | Mastodon `CustomEmoji` JSON array served as the custom emoji registry. Invalid JSON falls back to an empty registry. |

Keep JSON values compact enough for Worker vars. Prefer a future storage-backed content path for large or frequently edited content.

## Secret Handling
<!-- constrained-by ../../wrangler.toml.example -->

Use Cloudflare secrets for values that should not be committed.

```sh
wrangler secret put ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY
wrangler secret put RESEND_API_KEY
wrangler secret put WEB_PUSH_VAPID_PRIVATE_KEY
wrangler secret put TRANSLATION_API_KEY
```

`ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY` protects local account ActivityPub private keys at rest. Set it before running security backfills or creating production accounts. Use a long random value and keep the same value across deployments; rotating it requires decrypting and re-encrypting `account_private_keys`.

For `wrangler dev`, copy `.dev.vars.example` to `.dev.vars` to override secrets locally. It currently only documents an optional dev-only `AUTH0_CLIENT_ID` that takes precedence over the `wrangler.toml` value.

Keep machine-specific deployment copies in an ignored local file such as `wrangler.local.toml`, or in a private deployment environment. Do not commit API keys, private key material, or other secret values into templates or docs.

Run a dry-run before deployment.

```sh
nix develop -c just ci
```
