# Cloudflare Deploy Checklist

This repository is configured to run as a single Cloudflare Worker backed by D1 and R2.

For local commands and CI gates, see [Development Workflow](../getting-started/development.md).
For Worker bindings, environment variables, and secrets, see [Configuration Reference](../reference/configuration.md).

## Required Cloudflare Resources
<!-- constrained-by ../reference/configuration.md -->

- A D1 database bound as `DB`
- An R2 bucket bound as `MEDIA`
- A public custom domain for media objects, referenced by `MEDIA_PUBLIC_BASE_URL`

## Provisioning Steps

1. Create the D1 database.

   ```sh
   wrangler d1 create cfwdon
   ```

2. Create the R2 bucket.

   ```sh
   wrangler r2 bucket create cfwdon-media
   ```

3. Copy the generated D1 `database_id` into [`wrangler.toml`](../../wrangler.toml).

4. Replace placeholder vars in [`wrangler.toml`](../../wrangler.toml).

   At minimum, set production values for `INSTANCE_DOMAIN`, `SOURCE_URL`, `MEDIA_PUBLIC_BASE_URL`, `ACCESS_TEAM_DOMAIN`, and `ACCESS_AUD`.

5. Configure secrets that should not be committed.

   ```sh
   wrangler secret put RESEND_API_KEY
   wrangler secret put WEB_PUSH_VAPID_PRIVATE_KEY
   wrangler secret put TRANSLATION_API_KEY
   ```

   Only set optional secrets for features you enable.

6. Apply migrations to the remote D1 database.

   ```sh
   wrangler d1 migrations apply DB --remote
   ```

7. Run the full local gate.

   ```sh
   devbox run ci
   ```

8. Deploy the Worker.

   ```sh
   wrangler deploy
   ```

## Verification Gates

- `devbox run ci`
- `wrangler.toml` contains active `[[d1_databases]]` and `[[r2_buckets]]` bindings
- production vars do not contain placeholder values from the sample `wrangler.toml`
- `crates/cfwdon-core/src/config.rs` defaults match the binding names `DB` and `MEDIA`
- `crates/cfwdon-worker/src/runtime_config.rs` loads the expected instance and media environment variables
- `migrations/` contains the schema required by the Worker code

## Current Caveat

The repository is wired for Cloudflare Workers + D1 + R2, but the actual D1 database ID and R2 bucket are external Cloudflare resources and must exist before a real deployment can succeed.
