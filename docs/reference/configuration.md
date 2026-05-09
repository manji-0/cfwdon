# Configuration Reference

`cfwdon` は単一 Cloudflare Worker として動作し、D1 と R2 を binding で受け取る。実行時設定は主に `wrangler.toml` の `[vars]` と Cloudflare secrets から読む。

## Cloudflare Bindings

| Binding | Type | Required | Notes |
| --- | --- | --- | --- |
| `DB` | D1 database | Yes | `crates/cfwdon-core/src/config.rs` の default と Worker code が期待する database binding。 |
| `MEDIA` | R2 bucket | Yes | media upload body と profile media object を保存する bucket binding。 |

binding 名を変更する場合は、`wrangler.toml`、`cfwdon_core::AppConfig` の default、Worker の runtime code が一致していることを確認する。

## Public Instance Vars

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `INSTANCE_DOMAIN` | Recommended | `example.com` | WebFinger、actor URI、API payload に使う instance domain。 |
| `INSTANCE_NAME` | Recommended | `cfwdon` | instance name。 |
| `INSTANCE_DESCRIPTION` | Recommended | Cloudflare Workers based description | instance description。 |
| `SOURCE_URL` | Optional | unset | instance source URL。 |
| `INSTANCE_LANGUAGES` | Optional | app default | comma-separated language list。 |
| `CONTACT_EMAIL` | Optional | unset | instance contact account metadata。 |
| `INSTANCE_THUMBNAIL_URL` | Optional | unset | instance thumbnail URL。 |
| `ADMIN_EMAILS` | Optional | empty | comma-separated admin e-mail list for admin notifications. |
| `MEDIA_PUBLIC_BASE_URL` | Required for production media URLs | unset | R2 custom domain base URL. trailing slash is trimmed. |
| `WEB_UI_R2_PREFIX` | Required for Web UI serving | `phanpy` | R2 object prefix containing the self-hosted Phanpy bundle. `index.html` must exist under this prefix. |

`MEDIA_PUBLIC_BASE_URL` の domain は Cloudflare Access の保護対象から外す。Mastodon entity payload ではこの URL が正規 media URL として使われる。

## Web UI Bundle
<!-- constrained-by #cloudflare-bindings -->

Phanpy is the primary Web UI. It is served only from the configured R2 bucket, and `cfwdon` does not proxy `mastodon.social` or any other third-party instance at request time.

Build Phanpy as a static app for this instance, then upload the `dist` files under `WEB_UI_R2_PREFIX` so the Worker can read:

- `${WEB_UI_R2_PREFIX}/index.html`
- `${WEB_UI_R2_PREFIX}/manifest.webmanifest`
- `${WEB_UI_R2_PREFIX}/assets/...`
- any other static paths emitted by the Phanpy build, such as icons, service worker files, locale chunks, `compose/`, or `share`

For the sample configuration, that means objects such as `phanpy/index.html` and `phanpy/assets/index-....js` in the `MEDIA` bucket. The Worker injects a small cfwdon theme stylesheet into Phanpy HTML responses, but instance-specific Phanpy metadata should be set at build time with variables such as `PHANPY_CLIENT_NAME`, `PHANPY_WEBSITE`, and `PHANPY_DEFAULT_INSTANCE`.

## Access Authentication Vars

Protected API routes expect Cloudflare Access or an equivalent proxy to provide authenticated user context.

| Var | Required | Default / Behavior | Notes |
| --- | --- | --- | --- |
| `ACCESS_EMAIL_HEADER` | Required for protected API | `Cf-Access-Authenticated-User-Email` in sample config | Authenticated user e-mail header name。 |
| `ACCESS_JWT_HEADER` | Required for JWT validation | `Cf-Access-Jwt-Assertion` in sample config | Cloudflare Access JWT header name。 |
| `ACCESS_TEAM_DOMAIN` | Required for protected API | unset | Access issuer / team domain. missing with `ACCESS_AUD` blocks protected API. |
| `ACCESS_AUD` | Required for protected API | unset | Cloudflare Access audience. missing with `ACCESS_TEAM_DOMAIN` blocks protected API. |

`ACCESS_TEAM_DOMAIN` と `ACCESS_AUD` は実値を設定する。placeholder のまま本番 deploy しない。

## Policy And Instance Content Vars

| Var | Required | Notes |
| --- | --- | --- |
| `INSTANCE_EXTENDED_DESCRIPTION_HTML` | Optional | extended description endpoint body。 |
| `INSTANCE_EXTENDED_DESCRIPTION_UPDATED_AT` | Optional | extended description timestamp。 |
| `PRIVACY_POLICY_HTML` | Optional | privacy policy endpoint body。 |
| `PRIVACY_POLICY_UPDATED_AT` | Optional | privacy policy timestamp。 |
| `TERMS_OF_SERVICE_HTML` | Optional | terms endpoint body。 |
| `TERMS_OF_SERVICE_EFFECTIVE_DATE` | Optional | terms effective date。 |

HTML は Worker vars に入るため、サイズが大きい場合は Cloudflare の vars 制限を考慮する。

## Timeline Access Vars

各 value は `public`、`authenticated`、`disabled` のいずれか。未設定または不正値の場合は application default を使う。

| Var | Scope |
| --- | --- |
| `TIMELINES_ACCESS_LIVE_FEEDS_LOCAL` | local live/public feed access |
| `TIMELINES_ACCESS_LIVE_FEEDS_REMOTE` | remote live/public feed access |
| `TIMELINES_ACCESS_HASHTAG_FEEDS_LOCAL` | local hashtag feed access |
| `TIMELINES_ACCESS_HASHTAG_FEEDS_REMOTE` | remote hashtag feed access |
| `TIMELINES_ACCESS_TRENDING_LINK_FEEDS_LOCAL` | local link timeline access |
| `TIMELINES_ACCESS_TRENDING_LINK_FEEDS_REMOTE` | remote link timeline access |

## Push Notification Vars

| Var | Required | Notes |
| --- | --- | --- |
| `WEB_PUSH_VAPID_PUBLIC_KEY` | Required for advertised push support | Public VAPID key returned in instance/app shapes. |
| `WEB_PUSH_VAPID_PRIVATE_KEY` | Required for delivery | Store as a secret. push delivery fails without it. |
| `WEB_PUSH_VAPID_SUBJECT` | Required for delivery | VAPID subject, usually `mailto:admin@example.com` or an HTTPS URL. |

private key material should be configured with `wrangler secret put`, not committed in `wrangler.toml`.

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

If provider config is incomplete, translation provider integration is not enabled.

## JSON Content Vars

| Var | Required | Notes |
| --- | --- | --- |
| `ANNOUNCEMENTS_JSON` | Optional | JSON source for announcement documents. |
| `DONATION_CAMPAIGN_JSON` | Optional | JSON source for donation campaign document. |

Keep JSON values compact enough for Cloudflare Worker vars. For large or frequently edited content, prefer moving content management behind a future storage-backed path instead of expanding vars indefinitely.

## Secret Handling

Use Cloudflare secrets for values that should not be committed.

```sh
wrangler secret put RESEND_API_KEY
wrangler secret put WEB_PUSH_VAPID_PRIVATE_KEY
wrangler secret put TRANSLATION_API_KEY
```

Run a dry-run before deploy.

```sh
devbox run ci
```
