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

This starts `wrangler dev`. Public, unauthenticated routes are the easiest first smoke test. Routes that need D1, R2, Cloudflare Access headers, or secrets require the configuration described below.

## Cloudflare Resource Template
<!-- constrained-by ../operations/cloudflare-deploy.md#provisioning-steps -->
<!-- constrained-by ../reference/configuration.md#cloudflare-bindings -->

Start from the example config instead of editing from memory:

```sh
cp wrangler.toml.example wrangler.toml
```

Then create the required Cloudflare resources:

```sh
wrangler login
wrangler d1 create cfwdon
wrangler r2 bucket create cfwdon-media
```

Copy the D1 `database_id` into `wrangler.toml`, then replace the instance vars under `[vars]`. At minimum, set:

- `INSTANCE_DOMAIN`
- `SOURCE_URL`
- `MEDIA_PUBLIC_BASE_URL`
- `ACCESS_TEAM_DOMAIN`
- `ACCESS_AUD`

Keep `DB` and `MEDIA` as the binding names unless you also update the Worker defaults.

## Secrets
<!-- constrained-by ../reference/configuration.md#secret-handling -->

Only set secrets for the features you enable:

```sh
wrangler secret put RESEND_API_KEY
wrangler secret put WEB_PUSH_VAPID_PRIVATE_KEY
wrangler secret put TRANSLATION_API_KEY
```

Do not commit private keys, API tokens, or production Access audience values into documentation or templates.

## Database Migrations
<!-- constrained-by ../operations/cloudflare-deploy.md#provisioning-steps -->

Apply the checked-in D1 migrations before a real deploy:

```sh
wrangler d1 migrations apply DB --remote
```

For local iteration, use `wrangler d1 migrations apply DB --local` if you want the local D1 database to match the schema.

## Deploy
<!-- constrained-by ../operations/cloudflare-deploy.md#verification-gates -->

Before deploying:

```sh
devbox run ci
```

Then deploy:

```sh
wrangler deploy
```

After deployment, verify that public instance endpoints return your configured domain, media URLs use `MEDIA_PUBLIC_BASE_URL`, and protected routes are reachable through Cloudflare Access.

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

- `wrangler deploy --dry-run` fails with D1 or R2 binding errors: confirm `wrangler.toml` has `[[d1_databases]]` binding `DB` and `[[r2_buckets]]` binding `MEDIA`.
- Protected API routes reject requests: confirm Cloudflare Access sends `Cf-Access-Authenticated-User-Email` and `Cf-Access-Jwt-Assertion`, and that `ACCESS_TEAM_DOMAIN` plus `ACCESS_AUD` are real values.
- Media URLs point at the wrong host: set `MEDIA_PUBLIC_BASE_URL` to the public R2 custom domain and keep that domain outside Cloudflare Access.
- `wasm-bindgen` version errors: leave and re-enter `devbox shell`; the init hook installs the pinned `wasm-bindgen-cli` version.

## Summary
<!-- derived-from #repository-bootstrap -->
<!-- derived-from #cloudflare-resource-template -->
<!-- derived-from #deploy -->

A fresh clone should be able to enter `devbox shell`, run `devbox run ci`, copy `wrangler.toml.example`, create D1 and R2 resources, apply migrations, and deploy with `wrangler deploy`.
