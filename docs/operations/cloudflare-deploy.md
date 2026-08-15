# Cloudflare Deploy Checklist

This repository is configured to run as a single Cloudflare Worker backed by D1 and R2, with Web and admin UI files attached as Workers static assets.

For local commands and CI gates, see [Development Workflow](../getting-started/development.md).
For Worker bindings, environment variables, and secrets, see [Configuration Reference](../reference/configuration.md).

## Required Cloudflare Resources
<!-- constrained-by ../reference/configuration.md -->

- A D1 database bound as `DB`
- An R2 bucket bound as `MEDIA`
- A KV namespace bound as `REMOTE_DNS_CACHE` (remote hostname DoH validation cache)
- A KV namespace bound as `APP_CACHE` (account capability bits and other short-lived app cache)
- A public custom domain for media objects, referenced by `MEDIA_PUBLIC_BASE_URL`

## Provisioning Steps
<!-- constrained-by ../reference/configuration.md#public-instance-vars -->

1. Create the D1 database.

   ```sh
   wrangler d1 create cfwdon
   ```

2. Create the R2 bucket.

   ```sh
   wrangler r2 bucket create cfwdon-media
   ```

3. Create the remote DNS validation KV namespace.

   ```sh
   wrangler kv namespace create REMOTE_DNS_CACHE
   wrangler kv namespace create REMOTE_DNS_CACHE --preview
   ```

   Copy the returned ids into `[[kv_namespaces]]` for binding `REMOTE_DNS_CACHE`.

4. Create the app cache KV namespace.

   ```sh
   wrangler kv namespace create APP_CACHE
   wrangler kv namespace create APP_CACHE --preview
   ```

   Copy the returned ids into `[[kv_namespaces]]` for binding `APP_CACHE`.

5. Configure R2 CORS for the public instance origin.

   ```json
   {
     "rules": [
       {
         "allowed": {
           "origins": ["https://example.com"],
           "methods": ["GET"]
         }
       }
     ]
   }
   ```

   Save the policy as `r2-cors.json`, replace `https://example.com` with the `https://` origin for `INSTANCE_DOMAIN`, then apply it:

   ```sh
   npx wrangler r2 bucket cors set cfwdon-media --file r2-cors.json
   npx wrangler r2 bucket cors list cfwdon-media
   ```

   If the bucket custom domain is already serving cached objects, purge the media hostname after changing the CORS policy so cached assets pick up the new headers.

6. Copy the generated D1 `database_id` into [`wrangler.toml`](../../wrangler.toml).

7. Replace placeholder vars in [`wrangler.toml`](../../wrangler.toml).

   At minimum, set production values for `INSTANCE_DOMAIN`, `SOURCE_URL`, `MEDIA_PUBLIC_BASE_URL`, `AUTH0_DOMAIN`, `AUTH0_CLIENT_ID`, and `AUTH0_AUDIENCE`.

8. Configure secrets that should not be committed.

   ```sh
   wrangler secret put RESEND_API_KEY
   wrangler secret put WEB_PUSH_VAPID_PRIVATE_KEY
   wrangler secret put TRANSLATION_API_KEY
   ```

   Only set optional secrets for features you enable.

9. Apply migrations to the remote D1 database.

   ```sh
   wrangler d1 migrations apply DB --remote
   ```

10. Backfill deployed secret storage after migrations.
<!-- constrained-by ../reference/configuration.md#secret-handling -->

   Set `ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY` in the shell running the backfill to the same secret value configured in Cloudflare, then hash existing OAuth tokens and move account private keys into encrypted storage.

   ```sh
   ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY=... devbox run -- node scripts/backfill_security_secrets.mjs --database DB --remote
   ```

   Use `--dry-run` first if you want to inspect the generated SQL.

11. Run the full local gate.

    ```sh
    devbox run ci
    ```

12. Build the UI bundles so deploy does not upload the fallback HTML shells.

    ```sh
    (cd web-ui && pnpm install && pnpm run build)
    (cd admin-ui && npm install && npm run build)
    ```

    `wrangler deploy` stages `web-ui/dist` into `assets/app` and `admin-ui/dist` into `assets/admin`. Missing dist directories fall back to placeholder HTML.

13. Deploy the Worker.

    ```sh
    wrangler deploy
    ```

## Verification Gates

- `devbox run ci`
- `wrangler.toml` contains active `[[d1_databases]]`, `[[r2_buckets]]`, and `[assets]` bindings
- production vars do not contain placeholder values from the sample `wrangler.toml`
- `crates/cfwdon-core/src/config.rs` defaults match the binding names `DB`, `MEDIA`, `REMOTE_DNS_CACHE`, and `APP_CACHE`
- `wrangler.toml` `[assets] binding` is `ASSETS`
- `crates/cfwdon-worker/src/runtime_config.rs` loads the expected instance and media environment variables
- `migrations/` contains the schema required by the Worker code

## Current Caveat

The repository is wired for Cloudflare Workers + D1 + R2, but the actual D1 database ID and R2 bucket are external Cloudflare resources and must exist before a real deployment can succeed.
