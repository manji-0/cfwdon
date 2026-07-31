# Configuration Reference

`cfwdon` runs as a single Cloudflare Worker. It receives relational state through a D1 binding, media storage through an R2 binding, and most runtime settings through `wrangler.toml` `[vars]` plus Cloudflare secrets.

For a safe starting point, copy [`wrangler.toml.example`](../../wrangler.toml.example) to `wrangler.toml` and replace the placeholder values before deploying.

## Cloudflare Bindings

| Binding | Type | Required | Notes |
| --- | --- | --- | --- |
| `DB` | D1 database | Yes | Database binding expected by the Worker runtime and by the defaults in `crates/cfwdon-core/src/config.rs`. |
| `MEDIA` | R2 bucket | Yes | Bucket binding used for uploaded media bodies and profile media objects. |
| `REMOTE_DNS_CACHE` | KV namespace | Yes | Caches remote hostname DoH SSRF validation results for ActivityPub fetches. |
| `STREAM_HUB` | Durable Object | Yes | `StreamHub` binding for Mastodon streaming fan-out. |
| `INBOX_HOST` | Durable Object | Yes | `InboxHost` binding for per-remote-host inbox admission rate limiting (Phase C spike). |

If a binding name changes, keep `wrangler.toml`, `cfwdon_core::AppConfig` defaults, and Worker runtime code in sync.

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

Protected API routes validate Auth0-issued RS256 JWTs. By default, the Worker reads a Bearer token from `Authorization`, validates `iss` against `AUTH0_DOMAIN`, validates `aud` against `AUTH0_AUDIENCE`, fetches signing keys from `/.well-known/jwks.json`, and maps `AUTH0_EMAIL_CLAIM` to a local account e-mail. Browser login redirects use Auth0 Authorization Code with PKCE, return through `/oauth/auth0/callback`, set an HttpOnly local session cookie, and then continue the Mastodon OAuth consent flow.

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `AUTH0_JWT_HEADER` | Optional | `Authorization` | Header containing the Auth0 JWT. When this is `Authorization`, the Worker accepts standard `Bearer <token>` values. |
| `AUTH0_DOMAIN` | Required for protected API | unset | Auth0 tenant domain or issuer URL, for example `example.us.auth0.com`. |
| `AUTH0_CLIENT_ID` | Required for browser login redirects | unset | Auth0 application client ID used when redirecting `/oauth/authorize` flows to Auth0. |
| `AUTH0_AUDIENCE` | Required for protected API | unset | Auth0 API identifier expected in the JWT `aud` claim. |
| `AUTH0_EMAIL_CLAIM` | Required for protected API | `email` | String claim used to map the Auth0 user to `accounts.access_email`. |

Set real values for `AUTH0_DOMAIN`, `AUTH0_CLIENT_ID`, and `AUTH0_AUDIENCE`. Do not deploy production with placeholders. Auth0 domain, client ID, audience, and claim names are configuration values rather than secrets, so keep the canonical deployment values in `wrangler.toml`.

In the Auth0 application settings, include:

- Allowed Callback URLs: `https://<INSTANCE_DOMAIN>/oauth/auth0/callback`
- Allowed Logout URLs: `https://<INSTANCE_DOMAIN>`
- Allowed Web Origins: `https://<INSTANCE_DOMAIN>`
- Allowed Origins (CORS): `https://<INSTANCE_DOMAIN>`

For the complete Auth0 Dashboard setup, API audience selection, PKCE application requirements, and e-mail claim mapping, see [Auth0 Configuration Guide](../operations/auth0-configuration.md).

Cloudflare Access can protect private or operational Worker hostnames as an edge access gate, but it should not sit in front of the normal public Auth0 callback path. The current Worker does not treat Access JWTs as local user authentication. For the supported deployment boundary and policy setup, see [Cloudflare Access Configuration Guide](../operations/cloudflare-access-configuration.md).

## Policy And Instance Content Vars
<!-- constrained-by #Public Instance Vars -->

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
| `STREAM_HUB_PUBLIC_SHARD_COUNT` | Optional | `1` | When greater than `1`, each public timeline event fans out to every sharded hub (`public#0` … `public#N-1`), and WS/SSE clients sticky-route to one shard (account id, else `CF-Connecting-IP`, else `anon`). Clamped to `1`–`64`. |

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

Keep machine-specific deployment copies in an ignored local file such as `wrangler.local.toml`, or in a private deployment environment. Do not commit API keys, private key material, or other secret values into templates or docs.

Run a dry-run before deployment.

```sh
devbox run ci
```
