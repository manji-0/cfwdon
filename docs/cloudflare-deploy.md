# Cloudflare Deploy Checklist

This repository is configured to run as a single Cloudflare Worker backed by D1 and R2.

## Required Cloudflare Resources

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

3. Copy the generated D1 `database_id` into [`wrangler.toml`](../wrangler.toml).

4. Apply migrations to the remote D1 database.

   ```sh
   wrangler d1 migrations apply DB --remote
   ```

5. Deploy the Worker.

   ```sh
   wrangler deploy
   ```

## Verification Gates

- `cargo test -p cfwdon-worker --lib`
- `wrangler.toml` contains active `[[d1_databases]]` and `[[r2_buckets]]` bindings
- `crates/cfwdon-core/src/config.rs` defaults match the binding names `DB` and `MEDIA`
- `crates/cfwdon-worker/src/runtime_config.rs` loads the expected instance and media environment variables
- `migrations/` contains the schema required by the Worker code

## Current Caveat

The repository is wired for Cloudflare Workers + D1 + R2, but the actual D1 database ID and R2 bucket are external Cloudflare resources and must exist before a real deployment can succeed.
