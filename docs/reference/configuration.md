# Configuration Reference

`cfwdon` runs as a single Cloudflare Worker. It receives relational state through a D1 binding, media storage through an R2 binding, and most runtime settings through `wrangler.toml` `[vars]` plus Cloudflare secrets.

## Cloudflare Bindings

| Binding | Type | Required | Notes |
| --- | --- | --- | --- |
| `DB` | D1 database | Yes | Database binding expected by the Worker runtime and by the defaults in `crates/cfwdon-core/src/config.rs`. |
| `MEDIA` | R2 bucket | Yes | Bucket binding used for uploaded media bodies and profile media objects. |

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

Keep the `MEDIA_PUBLIC_BASE_URL` domain outside Cloudflare Access. Mastodon entity payloads use this value as the canonical media URL.

## Access Authentication Vars

Protected API routes expect Cloudflare Access, or an equivalent proxy, to provide authenticated user context.

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `ACCESS_EMAIL_HEADER` | Required for protected API | `Cf-Access-Authenticated-User-Email` in sample config | Header containing the authenticated user e-mail. |
| `ACCESS_JWT_HEADER` | Required for JWT validation | `Cf-Access-Jwt-Assertion` in sample config | Header containing the Cloudflare Access JWT. |
| `ACCESS_TEAM_DOMAIN` | Required for protected API | unset | Access issuer / team domain. Missing with `ACCESS_AUD` blocks protected API access. |
| `ACCESS_AUD` | Required for protected API | unset | Cloudflare Access audience. Missing with `ACCESS_TEAM_DOMAIN` blocks protected API access. |

Set real values for `ACCESS_TEAM_DOMAIN` and `ACCESS_AUD`. Do not deploy production with placeholders.

## Policy And Instance Content Vars

| Var | Required | Notes |
| --- | --- | --- |
| `INSTANCE_EXTENDED_DESCRIPTION_HTML` | Optional | Body returned by the extended description endpoint. |
| `INSTANCE_EXTENDED_DESCRIPTION_UPDATED_AT` | Optional | Extended description timestamp. |
| `PRIVACY_POLICY_HTML` | Optional | Privacy policy endpoint body. |
| `PRIVACY_POLICY_UPDATED_AT` | Optional | Privacy policy timestamp. |
| `TERMS_OF_SERVICE_HTML` | Optional | Terms endpoint body. |
| `TERMS_OF_SERVICE_EFFECTIVE_DATE` | Optional | Terms effective date. |

These HTML values live in Worker vars, so keep Cloudflare var size limits in mind. Move large or frequently edited content behind a storage-backed path later instead of expanding vars indefinitely.

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

Use Cloudflare secrets for values that should not be committed.

```sh
wrangler secret put RESEND_API_KEY
wrangler secret put WEB_PUSH_VAPID_PRIVATE_KEY
wrangler secret put TRANSLATION_API_KEY
```

Run a dry-run before deployment.

```sh
devbox run ci
```
