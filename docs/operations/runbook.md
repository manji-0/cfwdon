# Operations Runbook

This runbook is for the person running a `cfwdon` instance. It covers what holds state, backups and restore, what to watch, what to do when a federation or storage symptom shows up, and how to deploy. Every statement here was checked against the code or the docs it links; where a Cloudflare platform fact is not checked in this repository the text says so.

Provisioning is in the [Cloudflare Deploy Checklist](cloudflare-deploy.md). Bindings and vars are in the [Configuration Reference](../reference/configuration.md). Pre-launch checks are in the [Launch Checklist](launch-checklist.md).

## What Holds State
<!-- constrained-by ../reference/configuration.md#cloudflare-bindings -->

| Component | Binding | Holds | Loss means |
| --- | --- | --- | --- |
| D1 | `DB` | Everything relational: accounts, statuses, follows, OAuth tokens, encrypted account private keys, delivery queues (`outbox_deliveries`, `outbound_activities`), inbox dedup (`inbox_activities`), `background_jobs`, `media_metadata_backfill`, domain blocks. | Data loss. Back it up. |
| R2 | `MEDIA` | Uploaded media, avatars, headers, custom emoji images. | Broken media. D1 rows survive. |
| KV | `REMOTE_DNS_CACHE`, `APP_CACHE` | Caches only: DoH validation results, account capability bits, public endpoint payloads, trend lists. | Slower for a while. Rebuilt on demand. |
| Queue | `OUTBOX_PROCESS_QUEUE` | Kick messages `{ "reason": ... }`. Delivery state is in D1. | A delayed drain, at most until the next hourly cron. |
| Durable Object | `STREAM_HUB` | Live WebSocket and SSE fan-out for the streaming API. | Clients reconnect and fall back to polling. |
| Auth0 | `AUTH0_*` vars | Identity. The Worker only validates RS256 JWTs. | Nobody can sign in. See the [Auth0 guide](auth0-configuration.md). |

Secrets live in Cloudflare, not in git: `ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY` (required), and `RESEND_API_KEY`, `WEB_PUSH_VAPID_PRIVATE_KEY`, `TRANSLATION_API_KEY` for the features that use them. See [Secret Handling](../reference/configuration.md#secret-handling). Losing `ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY` makes the encrypted actor keys in a D1 backup unreadable, so store it outside the Cloudflare account too.

## Backups And Restore

### D1 Time Travel
<!-- derived-from #what-holds-state -->

D1 keeps a point-in-time history of the database. It is the fast path for "undo the last hour" and needs no setup.

```sh
wrangler d1 time-travel info DB
wrangler d1 time-travel restore DB --timestamp=<unix-seconds-or-iso>
```

Restore rewinds the whole database in place, so every write after the timestamp is lost, including statuses and follows that arrived since. `restore` prints the bookmark of the state it replaced; restoring to that bookmark undoes the restore. The retention window is a Cloudflare plan limit that this repository does not pin; read the current D1 documentation before relying on it. Time Travel lives in the same Cloudflare account as the database, so it does not cover losing the account.

### Scheduled D1 Backups
<!-- constrained-by ../../.github/workflows/d1-backup.yml -->

The [D1 backup workflow](../../.github/workflows/d1-backup.yml) exports the database daily at 03:43 UTC (and on demand), compresses and encrypts the dump with [age](https://age-encryption.org), uploads it to an S3-compatible bucket that should belong to another account or provider, verifies the stored size, and prunes old copies. The dump is the only form of the data that leaves the production account.

The workflow runs on schedule and on manual dispatch only. It never runs on push or pull request, so it cannot fail CI. A scheduled run on a repository where none of the settings below exist ends with a notice and passes. A manual run, or a run with only some of the settings, fails and names what is missing.

**Secrets and variables.** Set these under the repository's Actions settings.

| Name | Kind | Required | Meaning |
| --- | --- | --- | --- |
| `D1_BACKUP_CLOUDFLARE_API_TOKEN` | secret | yes | Cloudflare API token with the account permission D1 Edit and nothing else. The permission `wrangler d1 export` needs. Give it no other scope. |
| `D1_BACKUP_CLOUDFLARE_ACCOUNT_ID` | variable | yes | Account that owns the database. |
| `D1_BACKUP_AGE_RECIPIENTS` | variable | yes | One or more age public keys (`age1...`), separated by spaces, commas or newlines. This is the encryption key. Only the public half goes to GitHub. |
| `D1_BACKUP_S3_BUCKET` | variable | one of | Destination bucket. Set it for a bucket, or set the artifact retention below, or both. |
| `D1_BACKUP_S3_ENDPOINT` | variable | with a bucket | S3 API endpoint, for R2 `https://<account>.r2.cloudflarestorage.com`. |
| `D1_BACKUP_S3_ACCESS_KEY_ID`, `D1_BACKUP_S3_SECRET_ACCESS_KEY` | secrets | with a bucket | Credentials for that bucket, scoped to put, head, list and delete on it. |
| `D1_BACKUP_ARTIFACT_RETENTION_DAYS` | variable | one of | Also keep the encrypted file as a workflow artifact for this many days. Anyone who can read the repository's Actions can download it, and it is useless without the age identity. |
| `D1_BACKUP_S3_PREFIX` | variable | no | Key prefix, default `d1/`. |
| `D1_BACKUP_S3_REGION` | variable | no | Default `auto`, which R2 expects. |
| `D1_BACKUP_DATABASE_NAME`, `D1_BACKUP_DATABASE_ID` | variables | no | Override the database. By default both come from the `DB` entry of `wrangler.toml` (`database_name = "cfwdon-probe-5"` at the time of writing). |
| `D1_BACKUP_PRUNE`, `D1_BACKUP_KEEP_DAYS`, `D1_BACKUP_KEEP_MONTHS`, `D1_BACKUP_KEEP_LATEST` | variables | no | Retention: prune on by default, keep 35 days, the first backup of each of the last 12 months, and always the newest 7. |
| `D1_BACKUP_WRANGLER_VERSION` | variable | no | wrangler version the workflow installs. |

**Key custody.** Generate the identity on a machine you control with `age-keygen -o d1-backup.key`, put the printed `age1...` public key in `D1_BACKUP_AGE_RECIPIENTS`, and keep `d1-backup.key` offline in at least two places (a password manager and a printed or hardware-backed copy). Name two recipients if more than one person must be able to restore. A backup nobody can decrypt is no backup, so run the restore drill below before you trust the schedule. Rotating the key means adding the new public key to the variable; old backups still need the old identity.

**Bucket.** Create the bucket in a different Cloudflare account or at another provider so one compromised account cannot delete both the database and its backups. A bucket lock or object-lock rule is worth adding. Pruning logs a warning and carries on when the lock refuses a delete. Pruning only ever touches keys named `<prefix><database>-<UTC stamp>.sql.gz.age`; other objects in the bucket are left alone (see `scripts/d1_backup.mjs` and its tests in `scripts/lib/d1_backup.test.mjs`).

**What a dump contains.** Account private keys (encrypted with the Worker secret), OAuth token hashes, e-mail addresses, private posts and direct messages. Treat a decrypted dump as production data: never commit it, and decrypt only on an encrypted disk.

**Manual export.** Without the workflow:

```sh
wrangler d1 export DB --remote --output cfwdon-export.sql
```

### Restore From A Scheduled Backup
<!-- derived-from #scheduled-d1-backups -->

The drill never touches Cloudflare. It decrypts into a scratch local D1 database and reports counts and migrations the backup lacks.

```sh
scripts/d1_restore_check.sh d1-cfwdon-probe-5-20261109T034312Z.sql.gz.age ~/keys/d1-backup.key
```

It needs `age` and `wrangler` on `PATH` (`nix develop` provides `wrangler`). Run it after setting the schedule up and then every few months. For a real restore:

1. Put the instance in maintenance mode: `wrangler deploy --var CFWDON_MAINTENANCE:1`. HTTP answers 503 with `Retry-After`, and cron and queue invocations stop doing D1 work.
2. Create a new D1 database and load the decrypted dump with the split import described in [Moving The Database](cloudflare-deploy.md#moving-the-database) (`scripts/split_d1_export.py`; a whole-dump import fails with `D1_RESET_DO`).
3. Point `database_name`, `database_id` and `[placement] region` in `wrangler.toml` at the new database, then `wrangler d1 migrations apply DB --remote` for migrations newer than the backup, and deploy without the variable.
4. Federation catches up on its own: remote servers retry deliveries to you, and queued local deliveries resume. Statuses and follows made after the backup are gone; people may need to repost.

### R2 Media

R2 has no scheduled backup in this repository. Objects are written once and never rewritten except by the stored-media metadata backfill (see [Media Metadata Backfill](#media-metadata-backfill)). Deleting the bucket loses all uploads. If you need a copy, use `rclone` or another S3 client against the bucket's S3 API into a second account. D1 rows hold only object keys, so a restored D1 with an empty bucket serves broken media rather than erroring.

## Monitoring And Alerting

### Logs
<!-- derived-from ../../crates/cfwdon-worker/src/observability.rs -->

Workers Observability is on in `wrangler.toml` (`[observability]`, logs and traces at head sampling rate 1). The Worker writes one JSON object per line with `"source": "cfwdon"`. Filter on `event`, `outcome`, `component` and `level`.

| Event | Where | Meaning |
| --- | --- | --- |
| `api_request` | every `/api/`, `/oauth/` and `/.well-known/` request | `method`, `path`, `status`, `duration_ms`, `d1_query_count`, `d1_sql_ms_sum`. Turn it off with `CFWDON_API_REQUEST_LOG=0`. |
| `d1_slow_query` | any D1 query over 500 ms | Carries `query_name`, `statement_family` and the request `route`. |
| `outbox_queue_kick`, `outbox_queue_kick_failed`, `outbox_queue_kick_skipped` | queue producer and cron | A kick was enqueued, failed, or skipped. |
| `outbox_queue_processed`, `outbox_queue_idle`, `outbox_queue_processing_failed` | queue consumer | Batch result with `expanded`, `delivered`, `failed`. `processing_failed` makes the whole batch retry. |
| `outbox_expand_planned`, `outbox_expanded` | consumer | A generic outbox row was fanned out to per-inbox rows. |
| `outbox_delivery_delivered`, `outbox_delivery_failed`, `outbox_delivery_skipped` | consumer | Per-target result. `failed` carries `attempt`, `terminal`, `permanent` and `error`. `skipped` means a domain block. |
| `inbox_processed`, `inbox_processing_failed`, `inbox_signature_failed`, `inbox_accepted_no_targets` | inbox | Inbound activity outcomes. |
| `inbox_domain_suspended` | inbox | A sender on a blocked domain was refused with 403. |
| `inbox_stale_reclaim`, `background_job_stale_reclaim` | hourly cron | Work stuck `in_flight` or `running` for more than 15 minutes was released or failed. |
| `federation_retention_pruned` | hourly cron | Rows deleted by retention, with a count per table. |
| `media_metadata_backfill` | hourly cron | One stored object's outcome; see below. |
| `stream_hub_websocket` | StreamHub | Close handling. `handled=true` with `outcome=inactive_instance` or `deploy_reset` is expected; see [Stream Hub hibernation exceptions](cloudflare-deploy.md#stream-hub-hibernation-exceptions). |
| `metrics_dropped` | any invocation | Analytics Engine points were left out for lack of budget. |

Errors the cron swallows (for example "expired poll processing failed") are plain `console_error` lines from the `scheduled` handler in `crates/cfwdon-worker/src/lib.rs`; search for the phrase `failed:`.

### Analytics Engine Metrics
<!-- constrained-by ../../crates/cfwdon-worker/src/metrics.rs -->

The Worker can write a small set of data points to a Workers Analytics Engine dataset. It is optional. Without the binding the writes are no-ops and nothing is logged.

To enable it, add the binding and deploy. Analytics Engine must be enabled for the account first.

```toml
[[analytics_engine_datasets]]
binding = "METRICS"
dataset = "cfwdon_metrics"
```

The binding name `METRICS` is a constant in the worker crate. A commented copy of the block is in `wrangler.toml.example`.

| Index | Doubles | Blobs | Written |
| --- | --- | --- | --- |
| `http.request` | status, latency ms, D1 query count, D1 SQL ms | route class, method, status class | once per request |
| `federation.event` | none | event, outcome | with every federation log event above |
| `cron.task` | duration ms | task, `ok` or `error` | once per cron step |
| `queue.batch` | message count, duration ms | none | once per queue batch |
| `metrics.dropped` | dropped, written | invocation, index that lost most | once, when a budget ran out |

The route class is deliberately coarse (`api/v1/statuses`, `users`, `.well-known/webfinger`; anything unknown is `other`) so scanners cannot create endless labels. `cron.task` task names are `outbox_queue_kick`, `collection_approvals`, `expired_polls`, `scheduled_statuses`, `reclaim_background_jobs`, `background_jobs`, `reclaim_inbox`, `purge_mutes`, `purge_idempotency_keys`, `purge_remote_content`, `prune_federation_activities`, `instance_activity_cache`, `public_timeline_cache` (hourly) and `trending_tags`, `trending_statuses` (every six hours).

Cloudflare drops data points past 250 per Worker invocation without an error. Each invocation therefore writes at most 249 and spends the last on `metrics.dropped`. Concurrent requests in one isolate share the budget of the latest invocation start, so the cap is approximate. Query the dataset through the Analytics Engine SQL API; `index1` is the index, `doubleN` and `blobN` follow the positions above:

```sql
SELECT blob1 AS route, blob3 AS status_class, SUM(_sample_interval) AS requests,
       SUM(_sample_interval * double2) / SUM(_sample_interval) AS mean_ms
FROM cfwdon_metrics
WHERE index1 = 'http.request' AND timestamp > NOW() - INTERVAL '1' DAY
GROUP BY route, status_class ORDER BY requests DESC
```

### Outbound Delivery Health (D1)
<!-- derived-from #delivery-retry-policy -->

Delivery state is rows in D1, so SQL answers most questions. Run these with `wrangler d1 execute DB --remote --command "..."`. `outbox_deliveries` has one row per target inbox (plus generic rows that expand into them); `outbound_activities` holds directly addressed activities.

```sql
-- State of both queues.
SELECT 'outbox' AS src, state, COUNT(*) AS n FROM outbox_deliveries GROUP BY state
UNION ALL SELECT 'outbound', state, COUNT(*) FROM outbound_activities GROUP BY state;

-- Overdue queued targets: what the next queue run picks up.
SELECT COUNT(*) FROM outbox_deliveries
WHERE state = 'queued' AND next_attempt_at IS NOT NULL AND next_attempt_at < datetime('now');

-- Recently failed targets and why.
SELECT target_inbox, attempt_count, last_error, updated_at FROM outbox_deliveries
WHERE state = 'failed' ORDER BY updated_at DESC LIMIT 20;
```

The same lists are in the admin UI (`/admin`) and at `GET /api/cfwdon/admin/deliveries?state=failed`, which needs an Auth0 token whose roles include one of `AUTH0_ADMIN_ROLES`, or an account the config lists as admin.

### Queue Health

`wrangler queues` and the dashboard show backlog for `cfwdon-outbox-process`. The consumer is configured with `max_batch_size = 10`, `max_batch_timeout = 0`, `max_retries = 3` and `max_concurrency = 1`. A healthy queue is almost always empty, because each message is only a kick. A growing backlog with `outbox_queue_processing_failed` events means the consumer throws before it can claim rows (D1 or config trouble). Many `outbox_queue_idle` events are normal. Cron re-kicks the queue every hour when delivery work is due, so a lost kick delays delivery by at most that long.

### Cron Health

Two triggers: `17 * * * *` runs hourly maintenance (queue kick, remote collection approval revalidation, expired polls, due scheduled statuses, stale job and inbox reclaim, background jobs, mute and idempotency-key purge, remote content purge, federation retention, instance activity and public timeline caches, then the stored media backfill). `17 */6 * * *` refreshes trending tags and statuses. A step that fails logs a `console_error` and the rest still run. With the Analytics Engine binding, `cron.task` gives per-step durations; without it, use the invocation duration in the Workers dashboard. An unknown cron string logs `scheduled: unknown cron trigger` and does nothing, so check that `wrangler.toml` and the constants in `lib.rs` agree after changing a schedule.

### D1

- A query over 500 ms logs `d1_slow_query`.
- When the previous request's total SQL time passed 8 seconds, heavy public reads (public timeline, instance activity and trend endpoints) answer 503 with `Retry-After: 5` to shed load.
- A transient `Network connection lost` is retried once.
- Keep the Worker next to the D1 primary: `[placement] region` must match the colo in a remote query's `meta.served_by_colo` ([Worker And D1 Placement](../reference/configuration.md#worker-and-d1-placement)).
- Size and row counts: `wrangler d1 info DB`.

### Durable Objects, R2

`StreamHub` is a hibernating Durable Object. A deploy resets it and closes WebSockets; clients reconnect. That is expected and logged as `stream_hub_websocket` with `outcome=deploy_reset`. Alert on hibernation exceptions that have no handled `stream_hub_websocket` entry beside them. For R2, watch bucket size in the dashboard; `R2` operations also log as `cfwdon_operation` debug events.

### Suggested Alerts

Wire these in Cloudflare Notifications or whatever watches your logs.

- `GET /api/v1/instance` and `GET /.well-known/nodeinfo` from outside, every minute.
- `outbox_queue_processing_failed` more than a few times in an hour.
- Failed delivery rows growing (query above) or overdue queued rows not shrinking.
- `inbox_processing_failed` or `inbox_signature_failed` spikes (see [Inbox flood](#inbox-flood)).
- The backup workflow failing; GitHub emails the repository owner on a failed scheduled run.
- `metrics_dropped` appearing regularly, which means an invocation does more than the cap.

## Delivery Retry Policy
<!-- constrained-by ../../crates/cfwdon-domain/src/delivery.rs -->

Delivery follows Mastodon's `DeliveryWorker`: one attempt and up to 16 retries, 17 attempts in all. The delay before retry `n` is Sidekiq's `n^4 + 15 + rand(10) * (n + 1)` seconds, so the retries span about two days. A remote `Retry-After` on a 429 or 503 extends the delay, capped at 12 hours.

| Remote answer | Result |
| --- | --- |
| 2xx | delivered |
| 401, 408, 429 and 5xx, network errors | retry with backoff |
| other 4xx, and 501 | permanent failure, no retry |

After the last attempt, or on a permanent failure, the row's state becomes `failed` and stays in D1 with `last_error`. A claimed row stuck `in_flight` for more than 15 minutes (the invocation died) is put back to `queued`. Terminal rows (`delivered`, `failed`, and `expanded` generic rows) are deleted after `FEDERATION_ACTIVITY_RETENTION_DAYS`, default 30. Delivery to a domain blocked by the sender, or suspended on the instance, is skipped and logged as `outbox_delivery_skipped`.

## Common Incidents

### A Remote Server Is Down, Or Delivery Is Backing Up
<!-- derived-from #delivery-retry-policy -->

Queued rows for a dead host keep retrying on the schedule above and cost one failed POST each time. This is normal and needs no action; each row gives up after its 17 attempts. A retry that has come due runs on the next queue kick: any successful write request, a queue continuation, or the hourly cron. If the queue itself is the problem (large `queued` count, `outbox_queue_idle` absent, no `outbox_queue_processed`), check `outbox_queue_processing_failed` for the error, then kick the consumer by hand with `POST /internal/outbox/process` using a valid Auth0 token (it answers 202 when work is pending and `idle` otherwise). Each run claims a bounded batch and continues only while the previous batch made progress, so a stalled backlog cannot spin forever.

To retry failed rows after a remote comes back, use the admin UI or `POST /api/cfwdon/admin/deliveries/<id>/retry` (only rows in `failed` state qualify; the query parameter `source` is `outbox` or `outbound`, default `outbound`).

### Inbox Flood
<!-- constrained-by ../../crates/cfwdon-worker/src/inbox.rs -->

Signs: a burst of `inbox_processed`, `inbox_signature_failed` or `inbox_processing_failed`, slow D1. Inbound activities are deduplicated by `(actor_uri, activity_id)` in `inbox_activities`, so replays are cheap. To stop an abusive sender, add its domain as an instance domain block in the admin UI (`POST /api/cfwdon/admin/domain-blocks`); the inbox then answers 403 `forbidden domain` and logs `inbox_domain_suspended` before doing any work. Block rules are cached in the Worker; adding or removing a block invalidates the cache only in the isolate that handled the change, so another isolate may keep the old list briefly. Stuck inbox rows (claimed, never finished) are released by the hourly `inbox_stale_reclaim` or on demand with `POST /api/cfwdon/admin/inbox-activities/reclaim`.

### Domain Blocks

Instance domain blocks live in `instance_domain_blocks` and are managed under `/api/cfwdon/admin/domain-blocks` (list, create, delete) and in the admin UI. A blocked domain is refused at the inbox and skipped for outbound delivery. Per-account domain blocks (`account_domain_blocks`) also feed delivery skipping for that account. Removing a block does not replay what was refused.

### Background Jobs Stuck

`background_jobs` holds card unfurls, in-reply-to resolution, remote context fetch and remote status notifications. A job is retried with backoff up to 5 attempts, then `failed`. A `running` job older than 15 minutes is reclaimed hourly (back to `pending`, or `failed` after the last attempt). Inspect and act from the admin UI or `GET /api/cfwdon/admin/background-jobs`, `POST .../reclaim`, and `POST .../<id>/retry`.

### Media Bucket Growth

Nothing in the cron prunes media, so R2 size only grows with uploads. Find large objects in the dashboard, then delete the owning attachment through the API or by key, and purge the media hostname's Cloudflare cache so cached copies disappear.

### Media Metadata Backfill
<!-- constrained-by cloudflare-deploy.md#stripping-metadata-from-stored-media -->

Migration `127_media_metadata_backfill.sql` queued every stored object for an in-place metadata strip. Each hourly cron run, after everything else, handles up to 200 objects or 64 MiB. Each object logs `media_metadata_backfill` with an `outcome`:

| Outcome | Meaning | Action |
| --- | --- | --- |
| `stripped` | Rewritten without metadata. | none |
| `unchanged` | Nothing to strip. | none |
| `missing` | No R2 object for the key. | none; the row is dequeued |
| `deferred` | R2 read or write failed, or the object changed while read. | none; it stays queued |
| `too_large` | Over 40 MiB. | still carries its metadata; delete or replace by hand |
| `unstrippable` | A TIFF, which keeps its EXIF. | same |
| `unreadable` | Malformed file. | same |
| `abandoned` | Three runs started on it without an outcome (it kept ending the invocation). | same |

Progress: `SELECT COUNT(*) FROM media_metadata_backfill` on the remote database; it is 0 when done. After a long backfill, purge the media hostname cache, as [cloudflare-deploy.md](cloudflare-deploy.md#stripping-metadata-from-stored-media) describes.

### Access Misconfiguration Or Admin Lockout

Admin routes require an Auth0 token whose roles include one of `AUTH0_ADMIN_ROLES` (`fedi-admin` in the committed config), or an account the config marks as admin. If Cloudflare Access is in front, see the [Access guide](cloudflare-access-configuration.md) for which paths must stay open (ActivityPub, `/.well-known/`, the media domain). If an admin is locked out, check the role in Auth0 and the `AUTH0_ADMIN_ROLES` var before touching the database; see the [Auth0 guide](auth0-configuration.md).

### Key Compromise And Secret Rotation

- **Cloudflare API token or R2 keys:** revoke and reissue in the dashboard, then update the GitHub secrets for the backup workflow.
- **Auth0:** rotate the application settings in Auth0; the Worker holds no Auth0 secret, only the public client id, domain and audience.
- **`RESEND_API_KEY`, `WEB_PUSH_VAPID_PRIVATE_KEY`, `TRANSLATION_API_KEY`:** `wrangler secret put` the new value. Changing the VAPID key invalidates existing push subscriptions.
- **`ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY`:** rotating it requires decrypting and re-encrypting `account_private_keys` ([Secret Handling](../reference/configuration.md#secret-handling)); there is no tool in this repository for that, so do not change the value without a plan.
- **age identity:** see key custody above. Add a new recipient first; delete the old identity only after every backup you care about has been re-encrypted or has expired.

### Rolling Back A Bad Deploy

`wrangler rollback` returns the Worker to an earlier version. It does not undo D1 migrations. `scripts/check_migrations.py` validates that migrations apply to SQLite but does not prove they are backward compatible, so read the migration before rolling back past one. If a migration damaged data, use [D1 Time Travel](#d1-time-travel).

## Dead-Lettered Work
<!-- derived-from #queue-health -->

There is no dead-letter queue on `cfwdon-outbox-process`, on purpose.

The sibling project `tstodon` carries a dead-letter queue because its queue messages are the work itself, so a message dropped after its retries is work lost. In `cfwdon` a queue message is `{ "reason": "<why it was kicked>" }`. The state of each delivery lives in D1: the row, its `attempt_count`, `next_attempt_at` and `last_error`. A message that exhausts `max_retries = 3` and is dropped loses nothing; the rows are still `queued`, and the hourly cron kicks the queue again if any are due. The failure itself is already recorded as `outbox_queue_processing_failed` logs and `federation.event` data points, with the error text.

The things a DLQ would hold already exist, with more detail, in D1:

- A delivery that cannot succeed ends as a `failed` row with `last_error`, listed in the admin UI and retried from there.
- A job that cannot succeed ends as a `failed` row in `background_jobs`.
- An inbox activity stuck mid-processing is reclaimed by the hourly cron.

A DLQ would add one more queue to create, name and monitor, a second consumer, and a store for messages that carry no payload worth inspecting. Revisit this if messages ever gain payloads that exist nowhere else, if the consumer starts doing work that is not recorded in D1, or if you want an alert on "the queue drops messages" that the log event does not give you.

## Retention

| Data | Retention | Where |
| --- | --- | --- |
| Terminal `outbox_deliveries`, `outbound_activities`, processed `inbox_activities` | `FEDERATION_ACTIVITY_RETENTION_DAYS`, default 30; `0` keeps all. Up to 5,000 rows per table per hourly run. | `federation_retention.rs` |
| Public remote statuses and unreferenced remote actors | 7 days since publication, kept while followed, a follower's, or still referenced as a boost, quote or reply target. Up to 500 per run. | `relays.rs` |
| Expired mutes, status idempotency keys | Purged hourly. | `lib.rs` cron |
| `media_metadata_backfill` rows | Deleted as each object is handled. | `media_metadata_backfill.rs` |
| Workers Observability logs | Cloudflare's retention for your plan. | dashboard |

Failed deliveries older than the retention window are gone from D1, so investigate failures within 30 days.

## Upgrades

### Deploying A New Version
<!-- derived-from cloudflare-deploy.md#provisioning-steps -->

1. `nix develop -c just ci`. This runs formatting, clippy for native and wasm, tests, `check_migrations.py`, `check_query_plans.py`, `check_module_layers.py`, the Web UI checks and `wrangler deploy --dry-run`.
2. Apply migrations first: `wrangler d1 migrations apply DB --remote`.
3. Build the UIs and `wrangler deploy` (steps 13 and 14 of the [deploy checklist](cloudflare-deploy.md#provisioning-steps)).
4. Check `GET /api/v1/instance`, watch for `outbox_queue_processing_failed` and 5xx `api_request` events for ten minutes, and expect `stream_hub_websocket` `deploy_reset` as clients reconnect.

To pause every D1 read and write while you work on the database, deploy with `--var CFWDON_MAINTENANCE:1` and remove it afterwards.
