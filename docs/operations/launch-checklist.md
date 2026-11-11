# Launch Checklist

<!-- constrained-by ./cloudflare-deploy.md -->
<!-- constrained-by ./runbook.md -->

Go-live verification for a `cfwdon` instance on a real Cloudflare account, after the [Cloudflare Deploy Checklist](cloudflare-deploy.md) has been followed once. Each item says what to run and what you should see. Day-2 procedures (backups, alerts, incidents) are in the [Operations Runbook](runbook.md).

Throughout, `wrangler` runs from the repository root inside `nix develop`, and:

```sh
D=social.example.com          # INSTANCE_DOMAIN
O=https://$D
M=https://media.example.com   # MEDIA_PUBLIC_BASE_URL
```

## 1. Platform

### Resources Provisioned And Bound
<!-- constrained-by ../reference/configuration.md#cloudflare-bindings -->

- [ ] `wrangler d1 migrations list DB --remote` reports no migrations to apply.
- [ ] `wrangler d1 execute DB --remote --command "SELECT 1"` returns `meta.served_by_colo` in the region `[placement] region` names ([Worker And D1 Placement](../reference/configuration.md#worker-and-d1-placement)).
- [ ] `wrangler r2 bucket cors list cfwdon-media` shows your `https://$D` origin, and `curl -sI $M/<an uploaded object key>` answers 200 once any media exists.
- [ ] `wrangler queues info cfwdon-outbox-process` lists the Worker as producer and consumer. There is no dead-letter queue by design; see [Dead-Lettered Work](runbook.md#dead-lettered-work).
- [ ] The dashboard (Workers, `cfwdon`, Settings, Bindings) lists `DB`, `MEDIA`, `REMOTE_DNS_CACHE`, `APP_CACHE`, `OUTBOX_PROCESS_QUEUE`, `STREAM_HUB` and `ASSETS`, plus `METRICS` if you enabled [Analytics Engine metrics](runbook.md#analytics-engine-metrics).
- [ ] No `replace-with-` placeholder is left in `wrangler.toml`.
- [ ] The dashboard (Workers, `cfwdon`, Settings, Triggers) shows both crons: `17 * * * *` and `17 */6 * * *`.

### Secrets And Vars

- [ ] `wrangler secret list` includes `ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY`, and the optional secrets of features you enabled (`RESEND_API_KEY`, `WEB_PUSH_VAPID_PRIVATE_KEY`, `TRANSLATION_API_KEY`). A copy of the encryption key is stored outside Cloudflare.
- [ ] `curl -s $O/api/v1/instance | jq "{uri, source_url}"` shows your values, not `example.com`.
- [ ] Nothing secret is committed: `git grep -n "PRIVATE_KEY\|API_KEY" -- wrangler.toml` finds no values.

### Cloudflare Access Paths

If you put Cloudflare Access in front, follow the [Access guide](cloudflare-access-configuration.md). Federation, API and media paths must answer without the Access cookie:

```sh
curl -s -o /dev/null -w '%{http_code}\n' $O/api/v1/instance
curl -s -o /dev/null -w '%{http_code}\n' "$O/.well-known/webfinger?resource=acct:<user>@$D"
curl -s -o /dev/null -w '%{http_code}\n' $O/.well-known/nodeinfo
curl -s -o /dev/null -w '%{http_code}\n' -H 'Accept: application/activity+json' $O/users/<user>
```

- [ ] Each prints `200` (the webfinger and actor ones once the account exists).

### The First Admin
<!-- constrained-by ../reference/configuration.md#registration-vars -->

Auth0 and Access sign-ins never create accounts, and `REGISTRATIONS_MODE` defaults to `approved`, so the first account is created through the sign-up API and waits for approval like any other. Nobody can approve it from `/admin` yet, so the operator approves it from the command line.

1. Register an app that may create accounts, and get an app token for it:

   ```sh
   curl -s -X POST $O/api/v1/apps \
     -F client_name=bootstrap -F redirect_uris=urn:ietf:wg:oauth:2.0:oob -F scopes=write
   # Copy client_id and client_secret from the answer.
   curl -s -X POST $O/oauth/token \
     -F grant_type=client_credentials -F redirect_uri=urn:ietf:wg:oauth:2.0:oob \
     -F client_id=<client_id> -F client_secret=<client_secret> -F scope=write
   ```

2. Sign up with that app token. Use the e-mail address your Auth0 user signs in with, so the Auth0 sign-in finds the account:

   ```sh
   curl -s -X POST $O/api/v1/accounts -H "Authorization: Bearer <app access_token>" \
     -F username=alice -F email=alice@example.com -F password='<a long password>' \
     -F agreement=true -F locale=en
   ```

   The answer holds an access token. Until the account is approved, the API answers it with `403` and `{"error":"Your login is currently pending approval"}`.

3. Approve the account. The recipe deletes its `pending_account_approvals` row through `wrangler d1 execute`, which is what the admin approve endpoint does:

   ```sh
   just approve-account --remote alice              # or the e-mail address
   just approve-account --local alice               # the `just worker-dev` database
   ```

   It prints the matched account and `approved`, or `already approved; nothing to do` on a second run. Add `--database <name>` when the D1 binding is not `DB`.

4. Grant admin access: give the Auth0 user a role in `AUTH0_ADMIN_ROLES`, or list the account's e-mail in `ADMIN_EMAILS`.

`ADMIN_EMAILS` never approves a sign-up by itself, because nothing verifies the e-mail a sign-up gives. For the same reason, approve an account whose e-mail is in `ADMIN_EMAILS` only after you have checked that its owner controls that address: once approved, it is an admin.

- [ ] `curl -s $O/api/v1/instance | jq "{registrations, approval_required}"` shows `approval_required: true`, unless you set `REGISTRATIONS_MODE` on purpose.
- [ ] The first admin account is approved: `GET $O/api/v1/accounts/verify_credentials` with its token answers `200`.
- [ ] Sign in through Auth0 following [Validate the Flow](auth0-configuration.md#validate-the-flow). The e-mail claim is set and the local account links.
- [ ] A user with a role in `AUTH0_ADMIN_ROLES` opens `$O/admin`; a user without one gets 403 from `GET $O/api/cfwdon/admin/deliveries`.
- [ ] Later sign-ups show on the admin UI's pending accounts page, where admins approve or reject them.

### Posting With Media; Metadata Is Stripped

- [ ] Upload a photo with GPS EXIF through the Web UI, download the stored object from `$M`, and confirm `exiftool` shows no GPS tags.
- [ ] `SELECT COUNT(*) FROM media_metadata_backfill` on the remote database is 0, or falling each hour, with the remaining objects accounted for as in [Media Metadata Backfill](runbook.md#media-metadata-backfill).

### Crons And The Queue Consumer

- [ ] After the first `:17`, the logs show `outbox_queue_kick` only when delivery work was pending, and no `scheduled: unknown cron trigger`.
- [ ] Post a status. Within a minute the logs show `outbox_queue_kick`, then `outbox_queue_processed`.
- [ ] `POST $O/internal/outbox/process` with a valid Auth0 token answers `200` with `idle` or `202` with `queued`.

### Backups And Monitoring

- [ ] The [D1 backup workflow](../../.github/workflows/d1-backup.yml) is configured per [Scheduled D1 Backups](runbook.md#scheduled-d1-backups), and a manual run finished green and uploaded an object.
- [ ] `scripts/d1_restore_check.sh` decrypted that object and reported plausible counts ([Restore From A Scheduled Backup](runbook.md#restore-from-a-scheduled-backup)). The age identity is stored in two places.
- [ ] The alerts in [Suggested Alerts](runbook.md#suggested-alerts) exist.

## 2. Federation Smoke Test
<!-- derived-from #1-platform -->

Use an account on a Mastodon server you control or trust.

- [ ] From the remote server, search `@<user>@$D`; the profile loads and avatar and header come from `$M`.
- [ ] Follow in both directions. The follow resolves (`accepted`) and the logs show `inbox_processed` for the inbound `Accept` or `Follow`.
- [ ] Post from the remote account; it appears in your home timeline within seconds, live if the streaming connection is open.
- [ ] Post from `cfwdon`; it appears on the remote server, and the logs show `outbox_delivery_delivered` for that inbox.
- [ ] Reply, favourite, and boost across the servers; each shows on both sides.
- [ ] Block the remote domain through `POST /api/cfwdon/admin/domain-blocks`; its next delivery is refused with 403 and logs `inbox_domain_suspended`. Remove the block afterwards.
- [ ] Check failed deliveries stay failed rows rather than loops: `SELECT state, COUNT(*) FROM outbox_deliveries GROUP BY state` shows no growing `queued` count after a remote goes away.
