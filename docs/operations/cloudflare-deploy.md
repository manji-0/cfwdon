# Cloudflare Deploy Checklist

This repository is configured to run as a single Cloudflare Worker backed by D1, R2, two KV namespaces, a Queue, and a Durable Object, with Web and admin UI files attached as Workers static assets.

For local commands and CI gates, see [Development Workflow](../getting-started/development.md).
For Worker bindings, environment variables, secrets, and D1/Worker placement, see [Configuration Reference](../reference/configuration.md).

## Required Cloudflare Resources
<!-- constrained-by ../reference/configuration.md -->

- A D1 database bound as `DB`
- An R2 bucket bound as `MEDIA`
- A KV namespace bound as `REMOTE_DNS_CACHE` (remote hostname DoH validation cache)
- A KV namespace bound as `APP_CACHE` (account capability bits, public endpoint payloads, and trend lists)
- A Queue named `cfwdon-outbox-process`, bound as the producer `OUTBOX_PROCESS_QUEUE` (the same Worker is its consumer)
- A public custom domain for media objects, referenced by `MEDIA_PUBLIC_BASE_URL`

The `STREAM_HUB` Durable Object (class `StreamHub`), the `ASSETS` static assets binding, and the two cron triggers need no separate provisioning. They come from `wrangler.toml.example` (`[[durable_objects.bindings]]` plus the `[[migrations]]` entry tagged `v1`, `[assets]`, and `[triggers] crons`), and the Durable Object class is created on the first `wrangler deploy`.

## Provisioning Steps
<!-- constrained-by ../reference/configuration.md#public-instance-vars -->
<!-- constrained-by ../reference/configuration.md#worker-and-d1-placement -->

1. Create the D1 database in the same region you will pin the Worker to. Location is fixed at create time and cannot be changed later (see [Moving The Database](#moving-the-database)).

   ```sh
   wrangler d1 create cfwdon --location=apac
   ```

   Location hints are regional only. `apac` placed this instance's first database in Singapore (`SIN`); creating without a hint from Japan placed the current one in Osaka (`KIX`). Confirm with a remote query's `meta.served_by_colo`, then pair it with the nearest cloud region in `[placement] region` in [`wrangler.toml`](../../wrangler.toml) (`aws:ap-northeast-3` for `KIX`, `aws:ap-southeast-1` for `SIN`). See [Worker And D1 Placement](../reference/configuration.md#worker-and-d1-placement).

2. Create the R2 bucket.

   ```sh
   wrangler r2 bucket create cfwdon-media
   ```

3. Create the remote DNS validation KV namespace.

   ```sh
   wrangler kv namespace create REMOTE_DNS_CACHE
   wrangler kv namespace create REMOTE_DNS_CACHE --preview
   ```

   Copy the returned ids into `[[kv_namespaces]]` for binding `REMOTE_DNS_CACHE` (`id` and `preview_id`).

4. Create the app cache KV namespace.

   ```sh
   wrangler kv namespace create APP_CACHE
   wrangler kv namespace create APP_CACHE --preview
   ```

   Copy the returned ids into `[[kv_namespaces]]` for binding `APP_CACHE` (`id` and `preview_id`).

5. Create the outbound delivery queue.

   ```sh
   wrangler queues create cfwdon-outbox-process
   ```

   The `[[queues.producers]]` and `[[queues.consumers]]` entries in `wrangler.toml` already reference this queue name.

6. Configure R2 CORS for the public instance origin.

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

7. Copy the generated D1 `database_id` into [`wrangler.toml`](../../wrangler.toml).

8. Replace placeholder vars in [`wrangler.toml`](../../wrangler.toml).

   At minimum, set production values for `INSTANCE_DOMAIN`, `SOURCE_URL`, `MEDIA_PUBLIC_BASE_URL`, `AUTH0_DOMAIN`, `AUTH0_CLIENT_ID`, and `AUTH0_AUDIENCE`.

9. Configure secrets that should not be committed.

   ```sh
   wrangler secret put ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY
   wrangler secret put RESEND_API_KEY
   wrangler secret put WEB_PUSH_VAPID_PRIVATE_KEY
   wrangler secret put TRANSLATION_API_KEY
   ```

   `ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY` protects account ActivityPub private keys (see [Secret Handling](../reference/configuration.md#secret-handling)). Only set the other secrets for features you enable.

10. Apply migrations to the remote D1 database.

   ```sh
   wrangler d1 migrations apply DB --remote
   ```

11. Backfill deployed secret storage after migrations.
<!-- constrained-by ../reference/configuration.md#secret-handling -->

   Set `ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY` in the shell running the backfill to the same secret value configured in Cloudflare, then hash existing OAuth tokens and move account private keys into encrypted storage.

   ```sh
   ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY=... devbox run -- node scripts/backfill_security_secrets.mjs --database DB --remote
   ```

   Use `--dry-run` first if you want to inspect the generated SQL.

12. Run the full local gate.

    ```sh
    devbox run ci
    ```

13. Build the UI bundles so deploy does not upload the fallback HTML shells.

    ```sh
    (cd web-ui && pnpm install && pnpm run build)
    (cd admin-ui && npm install && npm run build)
    ```

    `wrangler deploy` stages `web-ui/dist` into `assets/app` and `admin-ui/dist` into `assets/admin`. Missing dist directories fall back to placeholder HTML.

14. Deploy the Worker.
<!-- derived-from ../getting-started/development.md#wrangler-rustc-path -->

    From `devbox shell`, or after `eval "$(devbox shellenv)"`:

    ```sh
    wrangler deploy
    ```

    The Worker `[build]` command pins repo rustup `wasm32-unknown-unknown` rustc and uses `worker-build --release` (wasm-opt). Local `wrangler dev` (`devbox run worker:dev`, or `WRANGLER_DEV=1`) is the only path that switches to `--dev`. If PATH rustc still lacks that target, wrap with `node scripts/with_wasm_rustc.mjs wrangler deploy`. On macOS do not use `devbox run -- wrangler deploy` (unsigned Xcode git).

    A deploy restarts Stream Hub Durable Objects and closes hibernating WebSockets.
    Clients reconnect; this is expected and is logged as
    `stream_hub_websocket` with `outcome=deploy_reset`, not as an application 5xx.

## Moving The Database
<!-- derived-from #provisioning-steps -->

D1 cannot change location, so moving means a new database and a copy. With about 12 MB this took under four minutes of downtime.

1. Create the new database and confirm `served_by_colo` with `SELECT 1`.
2. Deploy with `wrangler deploy --var CFWDON_MAINTENANCE:1`. HTTP answers 503 with `Retry-After`, and cron and queue invocations do no D1 work, so nothing writes to the old database. Avoid the `:17` cron minute.
3. `wrangler d1 export DB --remote --output export.sql`.
4. `python3 scripts/split_d1_export.py export.sql parts/`, then run each part in order with `wrangler d1 execute <new-name> --remote --file parts/<part> --yes`. Importing the whole dump at once fails with `D1_RESET_DO`, and its row order trips foreign keys.
5. Compare `COUNT(*)` for every table against the dump (D1 rejects long `UNION ALL` chains; use one `SELECT` of scalar subqueries per batch of tables).
6. Point `database_name` / `database_id` and `[placement] region` at the new database and `wrangler deploy` without the variable.
7. Re-enable read replication on the new database in the dashboard if it was on, and keep the old database until the move is settled.

## Stream Hub hibernation exceptions
<!-- derived-from #provisioning-steps -->

Hibernation eviction delivers `webSocketClose` with code 1006 and reason `this Durable Object instance is no longer active`. StreamHub does not echo that close. workerd rejects codes 1004, 1005, 1006, and 1015, and a close queued onto an already-dead hibernatable socket fails in the output pump after the handler has returned `Ok`. `HibernatableWebSocketCustomEvent::run` records that rejection as `$workers.outcome=exception` with `$metadata.origin=hibernatableWebSocket`. Swallowing the synchronous `WebSocket.close` error does not stop the pump.

Handled cases log `event=stream_hub_websocket` with `handled=true`, `outcome=inactive_instance` or `deploy_reset`, and `close_reply=skipped`. SSE reconnect and the D1 poll fallback are unchanged.

If the actor IoContext is already aborted when the event is delivered, the runtime still writes `$metadata.error` before user code runs. No handler return value clears that field or rewrites `$workers.outcome`. Alert on hibernatable exceptions that do not have the handled `stream_hub_websocket` log beside them.

## Verification Gates

- `devbox run ci`
- `wrangler.toml` contains active `[[d1_databases]]`, `[[r2_buckets]]`, `[[kv_namespaces]]` (`REMOTE_DNS_CACHE`, `APP_CACHE`), `[[queues.producers]]` / `[[queues.consumers]]`, `[[durable_objects.bindings]]` with its `[[migrations]]` entry, `[triggers] crons`, and `[assets]`
- production vars do not contain placeholder values from the sample `wrangler.toml`
- `crates/cfwdon-core/src/config.rs` defaults match the binding names `DB`, `MEDIA`, `REMOTE_DNS_CACHE`, `APP_CACHE`, and `STREAM_HUB`
- `wrangler.toml` `[assets] binding` is `ASSETS`
- `crates/cfwdon-worker/src/runtime_config.rs` loads the expected instance and media environment variables
- `wrangler.toml` `[placement] region` matches the D1 primary colo from `served_by_colo` (this instance: `SIN` → `aws:ap-southeast-1`)
- `migrations/` contains the schema required by the Worker code

## Current Caveat

The repository is wired for Cloudflare Workers + D1 + R2 + KV + Queues + Durable Objects, but the actual D1 database ID, R2 bucket, KV namespaces, and queue are external Cloudflare resources and must exist before a real deployment can succeed.
