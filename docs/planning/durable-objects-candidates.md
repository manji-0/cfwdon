# Durable Objects Candidate Features

<!-- constrained-by ../architecture/cfwdon-architecture.md#runtime-boundaries -->
<!-- constrained-by full-todo.md#activitypub-follow-up -->

Investigation of where Cloudflare Durable Objects (DOs) would help `cfwdon`, especially around Mastodon timeline streaming. This is a planning note, not an implementation commitment.

## Summary

Durable Objects fit features that need **long-lived connections**, **per-entity coordination**, or **exact-once-per-key scheduling**. The strongest fit today is **timeline streaming fan-out**. A strong federation fit is **per-remote-host inbox admission** (rate limit / backlog / ordered handoff), not separate public inbox URLs. Outbound ActivityPub delivery should stay on **Queues**. Per-entity schedules (scheduled statuses, poll expiry) are secondary DO candidates. Pure request/response Mastodon API and D1-backed reads should stay on the Worker + D1 path.

The Cloudflare **Agents SDK** sits on Durable Objects but targets assistant/session products (chat, MCP, email, workflows with UI). It is a poor default for Mastodon wire protocols and cfwdon’s Rust Worker core; see [Agents SDK Versus Bare Durable Objects](#agents-sdk-versus-bare-durable-objects).

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
| Session hub: `user`, `user:notification`, `direct`, `list` plus filter/announcement side effects | `user:{account_id}` | That account's clients |
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
- After hibernation, restore per-socket subscription state with `serializeAttachment` / tags (`stream=user`, `tag=rust`, `list=123`). A socket keeps a set of `(stream, tag, list)` keys so one hub can serve several channels and honour later `subscribe` messages.
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

## Agents SDK Versus Bare Durable Objects

Cloudflare **Agents** are Durable Objects with an opinionated TypeScript SDK on top (`Agent` → PartyServer `Server` → `DurableObject`). They add client state sync, `/agents/{name}/{instance}` routing, `@callable` RPC, richer scheduling, React hooks, chat/MCP/email helpers, and optional `AgentWorkflow` durable steps.

They are **not** a separate runtime from DOs. The question is whether the Agents abstraction helps more than it constrains.

### Prefer bare Durable Objects (or Queues) for cfwdon core

| Surface | Why Agents is a weak fit |
| --- | --- |
| Mastodon streaming (`/api/v1/streaming`) | Clients speak Mastodon SSE/WebSocket JSON, not `useAgent` / Agents protocol. Hibernation fan-out needs a thin protocol adapter, not Agents client sync. |
| Inbox host admission | Admission counters + Queue handoff are small coordination; Agents state sync / chat / routing add little. |
| Outbound delivery / WebPush | Already Queues-shaped; Agents would be overhead. |
| Rust Worker codebase | Agents SDK is a JS/TS (`agents`) package. cfwdon’s Worker is `workers-rs`. Using Agents implies a second TS Worker or rewriting surfaces. |

For StreamHub and `InboxHost`, implement **`#[durable_object]` in Rust** (or a small dedicated TS DO Worker if hibernation ergonomics demand it). Do not route Mastodon clients through `routeAgentRequest`.

### Where Agents *is* a better product fit

Agents shine when the unit of work is a **long-lived assistant or operator session** with tools, conversational UI, and resumable interaction—not when the unit is a Mastodon wire protocol endpoint.

| Case | Why Agents over raw DO |
| --- | --- |
| Instance operator / moderation copilot | Chat UI, tool calls (suspend user, inspect delivery DLQ, search reports), `setState` transcript, human-in-the-loop approvals via Workflows |
| Remote MCP server for cfwdon ops | `McpAgent` + OAuth-shaped tooling around D1/queue inspection without inventing a custom WS protocol |
| Email → draft status / report intake | Agents email routing + secure reply, then hand off to existing status/report APIs |
| Resumable admin workflows | Multi-step “review report → fetch remote actor → decide” with `AgentWorkflow` retries and progress to a browser client |
| Optional AI features on a web UI | Workers AI / external LLM with `AIChatAgent`, while Mastodon API stays on the Rust Worker |

These are **adjacent products** (ops console, MCP, email bridge), not replacements for streaming hubs or inbox admission.

### Decision rule

```text
Need Mastodon/ActivityPub wire compatibility or tiny keyed coordination?
  → bare Durable Object (Rust) or Queues/D1

Need conversational/tool-using session with first-party web/MCP/email clients?
  → Agents SDK (likely a separate TS Worker bound to the same Cloudflare account)
```

Avoid wrapping StreamHub in `Agent` “because WebSockets.” Hibernation WebSockets on a DO already cover that; Agents’ value is the **session/tool/UI layer**, which Mastodon clients do not use.

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

### Phase A — Streaming hub spike (highest value) — implemented

1. Added `StreamHub` Durable Object class and `STREAM_HUB` wrangler binding/migration.
2. Proxy WebSocket upgrades for all live channels (`user`, `user:notification`, `list`, `direct`, public variants, hashtag) through the DO; fall back to Worker poll on failure.
3. Soft-publish live events (never fail API writes):
   - Session hub `user:{account_id}` carries every authenticated channel: `user` status `update` / `delete` / `status.update`, `filters_changed`, announcement reaction/dismiss, local follower home fan-out (cap 200), remote status create/update/delete fan-out to local followers, `user:notification` events, `direct` events plus conversation `update` documents, and `list` events routed to the list owner
   - `user:notification` events: local favourite/reblog/follow/mention/reply/quote/poll/update; remote inbound favourite/reblog/follow/follow_request; remote Create mention/quote/status(notify)/reply; remote Update update/quoted_update
   - Open channels keep their own hubs: `public` / `public:remote` (+ media) and `hashtag:{tag}` (stream `hashtag` only) for local and remote status create/delete when visibility matches; local also uses `public:local` / `hashtag:local`
4. SSE for StreamHub-backed channels uses an internal hub WebSocket bridge with 30s D1 catch-up; falls back to 3s D1 poll if the hub is unavailable. Plain Worker WebSocket upgrade path unchanged.
5. Announcements: reaction/dismiss publish to `user:{account_id}`; config-only announcement body changes remain poll-only.
6. Remaining polish: escalate InboxHost to Queue handoff after D1 staging; alarm-based schedules vs cron.

### Phase B — Channel coverage — implemented

1. Public and hashtag hubs are proxied and published for local and remote status create/delete (remote uses public/remote hubs, not local-only); `list` and `direct` are served by the account session hub.
2. Delete / edit / filter / announcement-reaction publishers wired from mutation paths. Local edits publish `status.update` to the same channels as create.
3. SSE consumes StreamHub live events with D1 catch-up backup.
4. Public-channel sharding: `stream_hub_sharded_id_name` in `stream_hub.rs` (`public#0` … `public#N`); when `STREAM_HUB_PUBLIC_SHARD_COUNT` > 1, each public event fans out to the unsharded hub plus **all** shards. Only anonymous clients sticky-route to a shard via `CF-Connecting-IP` / `anon`.
5. Open-channel forwarding: authenticated clients always connect to their session hub. When such a socket subscribes to `public*` / `hashtag*`, the session hub registers itself on the open channel's unsharded hub (`POST /forward/register`), which relays matching events to it. Registrations are dropped lazily when a relay reports zero subscribers or fails, so disconnects need no explicit unregister.

### Phase C — Inbox host admission + schedule (optional)

1. **Landed (spike):** `InboxHost` Durable Object with `INBOX_HOST` binding and wrangler `v2` migration. Per-remote-host fixed-window admission (`60s` window, `120` admits) plus in-flight lease backlog (`max 32`) via `POST /admit` / `POST /release` on the DO. Leases carry an acquisition timestamp and expire after `30s`, so a Worker that dies between admit and release cannot wedge the host at the backlog limit; `/admit` reports whether it actually leased a slot and only that caller releases. Worker calls `admit_inbox_host_soft` before `begin_inbox_activity_if_needed` in verified inbox processing (**`lease: true`**, release after finish/replay skip) and on shared-inbox `AcceptedNoTargets` after signature verify (**`lease: false`**, rate-only). **AcceptedNoTargets** stays a cheap HTTP `202` without D1 dedupe slot. **InboxHost deny** (rate or backlog) returns HTTP `503` with `Retry-After`; do **not** queue work on deny—remotes must retry the original POST. DO/binding errors fail-open.
2. Poll sweep on cron landed; scheduled-status due publish on cron landed (hourly sweep via `process_due_scheduled_statuses_for_config`, internal `POST /internal/scheduled_statuses/process` for ops). Each row is claimed with an optimistic `claimed_at` update before publishing, so a concurrent sweep or a manual ops run cannot publish it twice; claims older than 5 minutes are retried. DO alarms still deferred. Still open: alarm-based scheduled status versus cron sweeps. Full **`INBOX_PROCESS_QUEUE` handoff is deferred** until a D1 staging table (`inbox_pending_work`) exists so the queue carries durable work keys only. Queue messages must be `{actor_uri, activity_id}` (well under Cloudflare Queues' **128KB** message limit); never enqueue large Create bodies by default—the Worker stages payload refs in D1 and consumers rehydrate from `inbox_activities` / staging.
3. If one remote host still dominates processing after the thin limiter + in-flight backlog, escalate to deferred Queue handoff while keeping public inbox URLs unchanged.

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
- At what backlog depth should `InboxHost` return 503 versus accept-and-queue? (current spike: deny at `in_flight >= 32` with `Retry-After: 5`)
- Should hot remote hosts shard by hash of `activity_id` or by actor URI?

## References

- Current streaming implementation: `crates/cfwdon-worker/src/meta_placeholder_routes.rs`, `crates/cfwdon-worker/src/streaming_types.rs`
- Outbox queue bindings: `wrangler.toml.example` (`OUTBOX_PROCESS_QUEUE`)
- Architecture open question on Queues: [cfwdon Architecture](../architecture/cfwdon-architecture.md)
- Cloudflare: [What are Durable Objects](https://developers.cloudflare.com/durable-objects/concepts/what-are-durable-objects/), [WebSockets + hibernation](https://developers.cloudflare.com/durable-objects/best-practices/websockets/), [Rules of Durable Objects](https://developers.cloudflare.com/durable-objects/best-practices/rules-of-durable-objects/)
- Rust support: `workers-rs` `DurableObject` trait / hibernation WebSocket API
