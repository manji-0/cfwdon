# ActivityPub → X Mirror Worker

Optional misc Cloudflare Worker that receives ActivityPub `Create` activities
in a dedicated bridge actor inbox and mirrors allowlisted remote notes to a
fixed X account via the X API v2.

This Worker is independent of the main `cfwdon` Rust Mastodon server. It does
not share D1, R2, or delivery queues with `cfwdon`.

## Behavior

1. Exposes WebFinger + a single Person actor (`ACTOR_USERNAME` on `INSTANCE_DOMAIN`).
2. Verifies HTTP Signatures on `POST /actors/<user>/inbox`.
3. For **public** `Create`/`Note` from an allowlisted `attributedTo` actor, enqueues a mirror job.
   Public means ActivityStreams `Public` is in `to` (Mastodon public). Unlisted (`Public` only in `cc`), followers-only, and DM notes are ignored.
4. Queue consumer posts plain text to `POST https://api.x.com/2/tweets` (OAuth 1.0a).
5. KV stores activity/object idempotency keys and `object_uri → tweet_id` mappings.

Outbound `Follow` to allowlisted actors is triggered manually via
`POST /admin/follow-allowlist` so followers delivery starts.

## Non-goals (v1)

- Media attachments
- Reply threading / boosts
- Delete or Update sync to X
- Writing into cfwdon timelines
- X → ActivityPub
- Multiple X accounts or interactive OAuth UI

## Setup

```sh
cd workers/ap-x-mirror
npm install
```

Create Cloudflare resources:

```sh
npx wrangler kv namespace create ap-x-mirror-store
npx wrangler kv namespace create ap-x-mirror-store --preview
npx wrangler queues create ap-x-mirror-jobs
```

Copy the KV ids into [`wrangler.jsonc`](wrangler.jsonc). Set `INSTANCE_DOMAIN`
and `ALLOWLIST_ACTOR_URIS` (comma-separated ActivityPub actor URIs). Attach a
custom domain route for the bridge host (WebFinger requires a stable authority).

### Generate the actor key

```sh
openssl genrsa 2048 | openssl pkcs8 -topk8 -nocrypt -out actor-private.pem
```

Store the PKCS#8 PEM as a Worker secret (do not commit it):

```sh
npx wrangler secret put ACTOR_PRIVATE_KEY_PEM < actor-private.pem
npx wrangler secret put ADMIN_TOKEN
npx wrangler secret put X_API_KEY
npx wrangler secret put X_API_SECRET
npx wrangler secret put X_ACCESS_TOKEN
npx wrangler secret put X_ACCESS_TOKEN_SECRET
```

X credentials are **OAuth 1.0a user tokens** for the single target account.
Posting requires an X API plan that allows `POST /2/tweets`.

### Deploy

```sh
npm run deploy
```

For local `wrangler dev`, copy [`.dev.vars.example`](.dev.vars.example) to `.dev.vars` and fill secrets.

Then bootstrap follows:

```sh
curl -X POST "https://<INSTANCE_DOMAIN>/admin/follow-allowlist" \
  -H "Authorization: Bearer <ADMIN_TOKEN>"
```

## Configuration

| Name | Where | Purpose |
| --- | --- | --- |
| `INSTANCE_DOMAIN` | vars | Bridge host / WebFinger domain |
| `ACTOR_USERNAME` | vars | Local actor username (default `bridge`) |
| `ACTOR_NAME` | vars | Display name |
| `ALLOWLIST_ACTOR_URIS` | vars | Comma-separated remote actor URIs to mirror |
| `APPEND_SOURCE_URL` | vars | Append note URL to the tweet (`true`/`false`, default true) |
| `MAX_TWEET_CHARS` | vars | Tweet length budget (default `280`) |
| `ACTOR_PRIVATE_KEY_PEM` | secret | PKCS#8 RSA private key |
| `ADMIN_TOKEN` | secret | Bearer token for `/admin/*` |
| `X_API_KEY` / `X_API_SECRET` | secrets | X consumer key pair |
| `X_ACCESS_TOKEN` / `X_ACCESS_TOKEN_SECRET` | secrets | X user access token pair |

## Routes

| Method | Path | Notes |
| --- | --- | --- |
| `GET` | `/health` | Liveness |
| `GET` | `/.well-known/webfinger?resource=` | JRD |
| `GET` | `/actors/<user>` | Actor document |
| `POST` | `/actors/<user>/inbox` | Signed ActivityPub inbox |
| `GET` | `/actors/<user>/outbox` | Empty OrderedCollection |
| `GET` | `/actors/<user>/followers` | Empty OrderedCollection |
| `GET` | `/actors/<user>/following` | Empty OrderedCollection |
| `POST` | `/admin/follow-allowlist` | Send Follow to allowlisted actors |

## Local check

```sh
npm run typecheck
npm run check
```

`wrangler deploy --dry-run` needs valid-looking KV ids in `wrangler.jsonc`; use
placeholders only for local typechecking if you have not created resources yet.
