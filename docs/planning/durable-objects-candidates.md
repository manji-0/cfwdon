# Durable Objects Candidate Features

<!-- constrained-by ../architecture/cfwdon-architecture.md#runtime-boundaries -->
<!-- constrained-by full-todo.md#activitypub-follow-up -->

Investigation of where Cloudflare Durable Objects (DOs) would help `cfwdon`, especially around Mastodon timeline streaming. This is a planning note, not an implementation commitment.

## Summary

Durable Objects fit features that need **long-lived connections**, **per-entity coordination**, or **exact-once-per-key scheduling**. The strongest fit today is **timeline streaming fan-out**. A strong federation fit is **per-remote-host inbox admission** (rate limit / backlog / ordered handoff), not separate public inbox URLs. Outbound ActivityPub delivery should stay on **Queues**. Per-entity schedules (scheduled statuses, poll expiry) are secondary DO candidates. Pure request/response Mastodon API and D1-backed reads should stay on the Worker + D1 path.

## Why Durable Objects Here

Cloudflare Workers are short-lived and isolate-scoped. DOs add:

| Capability | Relevance to cfwdon |
| --- | --- |
| Single-threaded coordination per named instance | One hub per stream channel or per account |
| Hibernatable WebSockets | Keep many idle Mastodon clients connected without burning duration |
| In-memory fan-out | Push one write event to many subscribers without each client polling D1 |
| Alarms | Wake one entity at a specific time (scheduled status, poll close) |
| Strongly consistent per-object storage | Rate-limit counters, connection metadata, short event buffers |

Do **not** use a single global DO. Shard by a natural atom (`user:{id}`, `hashtag:{name}`, `inbox-host:{domain}`, etc.). A global singleton becomes a throughput bottleneck (~hundreds of ops/sec per object).

## Current Streaming Baseline
<!-- derived-from ../../crates/cfwdon-worker/src/meta_placeholder_routes.rs -->

`GET /api/v1/streaming` already accepts SSE and WebSocket upgrades. Channel validation and auth exist for:

- Public: `public`, `public:media`, `public:local`, `public:local:media`, `public:remote`, `public:remote:media`
- Hashtag: `hashtag`, `hashtag:local` (require `tag`)
- Authenticated: `user`, `user:notification`, `list` (require `list`), `direct`

Live delivery today is **D1 poll every 3 seconds** inside the Worker invocation (`STREAMING_POLL_INTERVAL_SECS`). Each connection:

1. Holds `StreamingLoopState` (cursors, tracked status IDs, emitted event IDs).
2. Re-queries timeline/notification/list/direct batches from D1.
3. Recycles when poll budget is exhausted (`90` rounds / `200` subscription polls) or when the Workers subrequest limit is hit (`Too many API requests by single Worker invocation`).

That design is workable for small instances, but it scales poorly:

- Every open client repeatedly hits D1 even when idle.
- Latency is bounded by the poll interval (~3s), not by write time.
- Connections must reconnect after budget recycle.
- Public/hashtag streams duplicate the same D1 work per subscriber.
- Hibernation is unavailable on plain Worker WebSockets; idle connections keep the isolate busy.

## Candidate Ranking

### 1. Timeline streaming hubs — strong fit

**Problem.** Mastodon clients expect long-lived SSE/WebSocket streams with near-real-time `update`, `delete`, `status.update`, `notification`, `conversation`, `filters_changed`, and related events.

**DO shape.**

| Atom | `idFromName` example | Subscribers |
| --- | --- | --- |
| User home + filter/announcement side effects | `user:{account_id}` | That account's clients |
| User notifications | `user-notification:{account_id}` | That account's clients |
| Direct / conversations | `direct:{account_id}` | That account's clients |
| List | `list:{list_id}` | List owner clients |
| Public channel | `public`, `public:local`, … | Many anonymous clients |
| Hashtag | `hashtag:{normalized_tag}` | Tag watchers |

**Flow.**

```text
Write path (status create / delete / notify / filter change)
  Worker authenticates + writes D1 (source of truth)
  Worker builds Mastodon JSON payloads once
  Worker RPC/fetch-publishes to one or more StreamHub DOs

Read path (client connect)
  Worker validates channel + auth
  Worker proxies WebSocket upgrade to the StreamHub DO
  StreamHub accepts with Hibernation WebSocket API
  On publish: fan-out to sockets tagged for that channel
```

**Why DO wins over the current poll loop.**

- Event latency drops from poll interval to publish time.
- Idle clients hibernate; duration cost falls.
- Public/hashtag work is done once per event, not once per connection.
- Connection lifetime is no longer capped by Worker subrequest recycle.

**Design constraints.**

- Keep D1 as the durable source of truth. The DO is a **fan-out cache / session coordinator**, not the status store.
- Publish **pre-serialized Mastodon event payloads** from the Worker so the DO does not re-query D1 on every fan-out.
- Prefer WebSocket Hibernation for live clients. SSE can remain a Worker stream that either polls lightly or connects to the same hub via an internal protocol; hibernation is WebSocket-oriented.
- Hot public channels may need **hash sharding** (`public#0` … `public#N`) plus a thin publisher that fans to shards, once a single DO approaches ~500–1000 msgs/sec.
- After hibernation, restore per-socket subscription state with `serializeAttachment` / tags (`stream=user`, `tag=rust`, `list=123`).
- Clients that miss events while disconnected still catch up via REST timelines + `since_id`; optional short ring buffers in DO storage can reduce reconnect gaps but are not required for v1.

**Implementation note.** `workers-rs` already exposes `#[durable_object]`, `accept_web_socket`, and hibernation handlers. No Durable Object bindings exist in `wrangler.toml` yet.

### 2. Per-remote-host inbox admission — strong federation fit
<!-- derived-from ../../crates/cfwdon-worker/src/inbox.rs -->
<!-- derived-from ../../crates/cfwdon-worker/src/inbox/activity_store.rs -->

**Question.** Should inbound federation isolate “receivers” per remote server?

**Do not split the public ActivityPub inbox URL per remote host.** Remotes discover a fixed personal inbox (`/users/:username/inbox`) or shared inbox from actor documents. Inventing `/inbox/by-host/mastodon.social` forces non-standard discovery and breaks ordinary federation. Isolation belongs **behind** the existing inbox HTTP surface.

**Current baseline.**

- Signature verification and target resolution run in the Worker.
- Replay protection is D1 `inbox_activities` keyed by `(actor_uri, activity_id)` with in-flight / processed / release-on-failure semantics (also modeled in `cfwdon-domain`).
- Processing is **inline** in the request path: one floody remote host can consume Worker CPU/D1 budget that other hosts need.
- Dedupe is already per actor, not per host; there is no host-level backpressure or fair scheduling.

**DO shape that does fit.**

| Atom | `idFromName` example | Role |
| --- | --- | --- |
| Remote host admission | `inbox-host:{normalized_domain}` | Rate limit, backlog, fair queue for that host |
| Hot-host shard | `inbox-host:{domain}#{n}` | Split very large senders without a global bottleneck |
| Optional per-actor finer key | `inbox-actor:{actor_uri}` | Only if one actor on a quiet host is abusive |

**Recommended flow.**

```text
POST /inbox or /users/:name/inbox   (unchanged public URL)
  Worker: parse body, verify HTTP signature, resolve local targets
  Worker: derive host key from signing keyId / actor URI host
  Worker -> InboxHost DO: admit(activity_id, actor, payload ref)
       DO: enforce per-host rate / max in-flight / short backlog
       DO: return accept | 429/503 soft reject | duplicate
  On accept: enqueue durable work (Queue preferred) or continue process
  Keep D1 inbox_activities as durable replay source of truth
```

**Why DO helps here.**

- One noisy remote instance cannot starve every other sender’s CPU path as easily; backpressure is **per host**.
- Single-threaded DO admission gives exact counters and ordered handoff without a global lock.
- Matches the existing security follow-up (“rate limiting and abuse controls for shared inbox”).
- Complements actor-level D1 dedupe: host DO decides *whether to accept work now*; D1 still decides *whether this activity_id already ran*.

**What not to put in the DO.**

- Full Create/Announce/Like handlers, remote object fetches, and media work. Those stay Worker/Queue + D1.
- Authoritative replay state. Keep `inbox_activities` in D1 so DO eviction/hibernation cannot lose processed IDs.
- Public URL routing keyed by remote domain.

**Host key choice.** Prefer the host of the verified signing `keyId` (or verified actor URI) after signature success, not the TCP peer IP alone. Shared hosting and reverse proxies make IP-only keys weak; unsigned requests must still fail closed before any host probe.

**When this is worth building.**

- Shared-inbox abuse or retry storms from one large instance are observed.
- Inline processing latency under load starts rejecting healthy remotes.
- Operators want host-scoped 429/503 without blocking the whole instance.

Until then, a thinner `rl:inbox-host:{domain}` limiter DO (counters only, no backlog) or even Cache/KV soft limits may be enough. Full per-host queue DOs are the escalation path.

### 3. Per-key rate limiting / abuse control — strong secondary fit

`full-todo.md` calls for rate limits on shared inbox and expensive remote resolution. DOs fit **keyed** limiters; host inbox admission above is the federation-specialized form of the same idea.

| Key | Purpose |
| --- | --- |
| `rl:ip:{cf-connecting-ip}` | Client API / streaming connect abuse |
| `rl:inbox-host:{domain}` | Shared-inbox flood from one remote host (thin limiter; see §2 for full admission) |
| `rl:actor:{actor_id}` | Per remote actor delivery/inbox pressure |
| `rl:account:{account_id}` | Authenticated API write bursts |

Use sliding or fixed windows in DO storage. Avoid one global limiter DO.

KV or Cache API can approximate soft limits, but DOs give stronger per-key consistency when rejecting abuse must be exact.

### 4. Per-entity alarms — selective fit

| Feature | Current approach | DO alarm alternative |
| --- | --- | --- |
| Scheduled statuses | D1 rows + cron / internal sweep | `scheduled-status:{id}` alarm at `scheduled_at` |
| Expired polls | D1 queue + cron/internal route | `poll:{id}` alarm at expiry |
| Outbox retry backoff | Queues + D1 `attempt_count` / next-attempt | Prefer Queues; DO alarms add little |

Alarms help when work is **sparse and time-exact** (one status fires once). Cron sweeps remain fine while volume is low. Prefer introducing alarms only after streaming hubs prove DO operational cost is acceptable.

### 5. Presence / multi-device coordination — optional later

If multiple devices for one account should share markers, streaming subscriptions, or “which device got the push,” a `presence:{account_id}` DO can coordinate. Mastodon markers already live in D1 and do not need DO consistency today. Defer until a concrete multi-device product need appears.

## Poor Fits (Prefer Other Primitives)

| Feature | Prefer | Why |
| --- | --- | --- |
| Separate public inbox URL per remote host | Keep fixed shared/personal inbox | Breaks ActivityPub discovery; remotes will not learn custom per-host paths |
| Outbound ActivityPub delivery | Cloudflare Queues (already wired as `OUTBOX_PROCESS_QUEUE`) | High fan-out, independent HTTP deliveries, retry/backoff already modeled in D1 |
| WebPush send | Queues / `waitUntil` | Per-subscription HTTP posts; no shared session state |
| Stateless Mastodon REST | Worker + D1 | No long-lived coordination |
| Media upload / R2 | Worker + R2 | Object storage, not session coordination |
| Remote DNS SSRF cache | KV (`REMOTE_DNS_CACHE`) | Already a cache problem |
| Full timeline materialization | D1 | Relational queries, pagination, visibility rules stay in SQL |
| Authoritative inbox replay ledger | D1 `inbox_activities` | Must survive DO eviction; already actor+activity keyed |

Architecture docs already ask whether delivery should move further onto Queues. That remains the right direction; DOs should not replace the outbox consumer.

## Proposed Phased Approach

### Phase A — Streaming hub spike (highest value)

1. Add a `StreamHub` Durable Object class and wrangler binding/migration.
2. Proxy WebSocket upgrades for one authenticated channel (`user` or `user:notification`) through the DO with hibernation.
3. On local status create / notification insert, publish a prebuilt event to the hub.
4. Keep REST timelines and D1 writes unchanged; DO is additive.
5. Measure reconnect behavior, hibernation, and publish latency before expanding channels.

### Phase B — Channel coverage

1. Add `list`, `direct`, public, and hashtag hubs.
2. Wire delete / edit / filter / conversation side-effect publishers from existing mutation paths.
3. Keep SSE either as thin Worker poll or as a second consumer of the same publish bus.
4. Add public-channel shard plan if metrics show hotspots.

### Phase C — Inbox host admission + schedule (optional)

1. Start with thin per-host / per-IP DO rate limiters in front of shared inbox and remote fetch.
2. If one remote host still dominates processing, escalate to `InboxHost` admission DO + Queue handoff while keeping public inbox URLs unchanged.
3. Evaluate alarm-based scheduled status / poll expiry versus cron sweeps.

## Decision Criteria Before Committing

Adopt DOs for streaming when at least two of these are true:

- Operators care about sub-second timeline updates for connected clients.
- Open streaming connections cause measurable D1 load or frequent recycle disconnects.
- More than a handful of concurrent subscribers share the same public/hashtag channel.

Stay on Worker poll streaming when:

- Instance size stays tiny and 3s latency is acceptable.
- Engineering cost of publish hooks across create/delete/notify paths outweighs benefit.
- SSE-only clients dominate and WebSocket hibernation would not reduce cost enough.

## Open Questions

- Should public streams shard by channel only, or also by geographic colo affinity later?
- How much event history should a hub buffer for reconnect gap-fill versus forcing REST catch-up?
- Publish hooks: inline after D1 commit, or enqueue a “stream-publish” Queue message that wakes hubs?
- Keep streaming logic in `meta_placeholder_routes.rs`, or extract a `streaming/` module before DO work?
- Separate DO Worker script vs same Worker export — same Worker is simpler for Rust/`workers-rs` initially.
- For inbox host keys: prefer signing `keyId` host, actor URI host, or both with mismatch rejection?
- At what backlog depth should `InboxHost` return 503 versus accept-and-queue?
- Should hot remote hosts shard by hash of `activity_id` or by actor URI?

## References

- Current streaming implementation: `crates/cfwdon-worker/src/meta_placeholder_routes.rs`, `crates/cfwdon-worker/src/streaming_types.rs`
- Outbox queue bindings: `wrangler.toml.example` (`OUTBOX_PROCESS_QUEUE`)
- Architecture open question on Queues: [cfwdon Architecture](../architecture/cfwdon-architecture.md)
- Cloudflare: [What are Durable Objects](https://developers.cloudflare.com/durable-objects/concepts/what-are-durable-objects/), [WebSockets + hibernation](https://developers.cloudflare.com/durable-objects/best-practices/websockets/), [Rules of Durable Objects](https://developers.cloudflare.com/durable-objects/best-practices/rules-of-durable-objects/)
- Rust support: `workers-rs` `DurableObject` trait / hibernation WebSocket API
