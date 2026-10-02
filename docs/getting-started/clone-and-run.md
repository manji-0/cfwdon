# Clone And Run

This guide is for someone starting from a fresh clone who wants to bring up a `cfwdon` development environment and prepare a first Cloudflare deployment.

## Repository Bootstrap
<!-- constrained-by ./development.md -->

1. Clone the repository and enter it.

   ```sh
   git clone https://github.com/manji-0/cfwdon.git
   cd cfwdon
   ```

2. Start the pinned toolchain shell.

   ```sh
   devbox shell
   ```

3. Run the local validation gate.

   ```sh
   devbox run ci
   ```

The first `devbox shell` may install Rust, `wasm32-unknown-unknown`, `wrangler`, `worker-build`, `wasm-bindgen-cli`, and related build tools.

## Local Worker Smoke Test
<!-- constrained-by ./development.md#local-worker -->

Run the Worker locally with:

```sh
devbox run worker:dev
```

This builds `web-ui`, applies pending local D1 migrations, and starts `wrangler dev` (a `--instance` or `--remote` run skips the local migration step; see [Development Workflow](development.md#local-worker)). Public, unauthenticated routes are the easiest first smoke test. Routes that need D1, R2, Auth0 tokens, or secrets require the configuration described below.

## Cloudflare Resource Template
<!-- constrained-by ../operations/cloudflare-deploy.md#provisioning-steps -->
<!-- constrained-by ../reference/configuration.md#cloudflare-bindings -->
<!-- constrained-by ../reference/configuration.md#worker-and-d1-placement -->

Start from the example config instead of editing from memory:

```sh
cp wrangler.toml.example wrangler.toml
```

Then create the required Cloudflare resources:

```sh
wrangler login
wrangler d1 create cfwdon --location=apac
wrangler r2 bucket create cfwdon-media
wrangler kv namespace create REMOTE_DNS_CACHE
wrangler kv namespace create REMOTE_DNS_CACHE --preview
wrangler kv namespace create APP_CACHE
wrangler kv namespace create APP_CACHE --preview
wrangler queues create cfwdon-outbox-process
```

`--location=apac` places the D1 primary in Asia-Pacific (often Singapore, not Tokyo). It cannot be moved later. Check `meta.served_by_colo` on a remote query, then keep `[placement] region` next to that colo (`SIN` → `aws:ap-southeast-1`). Other regions: [Worker And D1 Placement](../reference/configuration.md#worker-and-d1-placement).

Copy the D1 `database_id` and the KV namespace ids (and preview ids) into `wrangler.toml`, then replace the instance vars under `[vars]`. At minimum, set:

- `INSTANCE_DOMAIN`
- `SOURCE_URL`
- `MEDIA_PUBLIC_BASE_URL`
- `AUTH0_DOMAIN`
- `AUTH0_CLIENT_ID`
- `AUTH0_AUDIENCE`

The template also declares the `STREAM_HUB` Durable Object (class `StreamHub`, created by the `[[migrations]]` entry on the first `wrangler deploy`), the `OUTBOX_PROCESS_QUEUE` producer and consumer for `cfwdon-outbox-process`, the `ASSETS` static assets binding, two cron triggers, and `[placement]`. Keep these as they are. Keep `DB`, `MEDIA`, `REMOTE_DNS_CACHE`, `APP_CACHE`, and `STREAM_HUB` as the binding names unless you also update the defaults in `crates/cfwdon-core/src/config.rs`; `OUTBOX_PROCESS_QUEUE` and `ASSETS` are fixed by the Worker code.

Configure the matching Auth0 application with allowed callback URL `https://<INSTANCE_DOMAIN>/oauth/auth0/callback` and allowed logout URL `https://<INSTANCE_DOMAIN>`.

## Secrets
<!-- constrained-by ../reference/configuration.md#secret-handling -->

`ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY` protects account ActivityPub private keys; set the other secrets only for the features you enable:

```sh
wrangler secret put ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY
wrangler secret put RESEND_API_KEY
wrangler secret put WEB_PUSH_VAPID_PRIVATE_KEY
wrangler secret put TRANSLATION_API_KEY
```

Do not commit private keys, API tokens, or other secret values into documentation or templates. Auth0 domain, client ID, audience, and claim names can live in `wrangler.toml`.

## Database Migrations
<!-- constrained-by ../operations/cloudflare-deploy.md#provisioning-steps -->

Apply the checked-in D1 migrations before a real deploy:

```sh
wrangler d1 migrations apply DB --remote
```

`devbox run worker:dev` already applies pending local migrations before it starts. Run `wrangler d1 migrations apply DB --local` yourself only when you start `wrangler dev` another way.

After migrating a deployed database, run the secret backfill described in [Cloudflare Deploy Checklist](../operations/cloudflare-deploy.md#provisioning-steps) if it already holds accounts or OAuth tokens.

## Deploy
<!-- constrained-by ../operations/cloudflare-deploy.md#verification-gates -->
<!-- derived-from ./development.md#wrangler-rustc-path -->

Before deploying:

```sh
devbox run ci
```

`devbox run ci` builds `web-ui` but not `admin-ui`. Build `admin-ui` too, or the deploy ships the placeholder page at `/admin`:

```sh
(cd admin-ui && npm install && npm run build)
```

Then deploy from `devbox shell` (or after `eval "$(devbox shellenv)"`):

```sh
wrangler deploy
```

`scripts/build_worker.mjs` pins repo rustup `wasm32-unknown-unknown` rustc. If PATH rustc is still a host or Nix compiler without wasm32:

```sh
node scripts/with_wasm_rustc.mjs wrangler deploy
```

On macOS do not use `devbox run -- wrangler deploy` (unsigned Xcode git).

After deployment, verify that public instance endpoints return your configured domain, media URLs use `MEDIA_PUBLIC_BASE_URL`, protected routes accept Auth0-issued access tokens, and browser login returns through `/oauth/auth0/callback`.

## Contributor Loop
<!-- derived-from ./development.md#common-commands -->

Use these commands during normal development:

```sh
devbox run fmt
devbox run check
devbox run test
devbox run ci
```

When route behavior changes, regenerate the Mastodon compatibility docs:

```sh
python3 scripts/generate_mastodon_api_compat.py
```

## Troubleshooting
<!-- derived-from ../reference/configuration.md -->

- `wrangler deploy --dry-run` fails with binding errors: confirm `wrangler.toml` has `[[d1_databases]]` binding `DB`, `[[r2_buckets]]` binding `MEDIA`, `[[kv_namespaces]]` bindings `REMOTE_DNS_CACHE` and `APP_CACHE` with real ids, and the `[[queues.producers]]` / `[[durable_objects.bindings]]` entries from the template.
- `/app` or `/admin` shows the fallback placeholder page (Japanese text saying the assets have not been built): build the matching UI (`web-ui` or `admin-ui`) before `wrangler deploy`. `worker:dev` rebuilds only `web-ui`.
- Protected API routes reject requests: confirm the request sends `Authorization: Bearer <Auth0 access token>`, and that `AUTH0_DOMAIN` plus `AUTH0_AUDIENCE` match the token `iss` and `aud` claims.
- Browser login does not return from Auth0: confirm the Auth0 application allows `https://<INSTANCE_DOMAIN>/oauth/auth0/callback`.
- Media URLs point at the wrong host: set `MEDIA_PUBLIC_BASE_URL` to the public R2 custom domain.
- `wasm-bindgen` version errors: leave and re-enter `devbox shell`; the init hook installs the pinned `wasm-bindgen-cli` version.
- `wrangler` / `worker-build` fail with a missing `wasm32-unknown-unknown` target: a host or Nix `rustc` is ahead of repo rustup. Use `devbox shell` then `wrangler deploy`, or `node scripts/with_wasm_rustc.mjs wrangler deploy`. On macOS do not use `devbox run -- wrangler`.

## Summary
<!-- derived-from #repository-bootstrap -->
<!-- derived-from #cloudflare-resource-template -->
<!-- derived-from #deploy -->

A fresh clone should be able to enter `devbox shell`, run `devbox run ci`, copy `wrangler.toml.example`, create the D1, R2, KV, and queue resources, apply migrations, and deploy with `wrangler deploy`.
