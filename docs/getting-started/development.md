# Development Workflow

`cfwdon` pins its Rust workspace and Cloudflare Worker tooling through `devbox`. Run commands from the repository root unless a command says otherwise.

For a fresh clone and first deploy path, see [Clone And Run](clone-and-run.md).

## Prerequisites

- `devbox`
- `wrangler` authentication when working with Cloudflare Workers, D1, or R2
- Git for repository history

`devbox.json` provides:

- Rust stable toolchain
- `wasm32-unknown-unknown` target
- `rustfmt`
- `wrangler`
- `worker-build`
- `wasm-bindgen-cli`
- `binaryen`
- `jq`

## Common Commands

```sh
devbox shell
```

```sh
devbox run fmt
```

```sh
devbox run fmt:check
```

```sh
devbox run check
```

```sh
devbox run test
```

```sh
devbox run ci
```

`devbox run ci` runs the full local gate (web UI + server). Split targets are also available:

```sh
devbox run ci:web-ui
devbox run ci:server
```

GitHub Actions runs `web-ui` and `server` on separate runners in parallel. The aggregate `CI / ci` job still reports overall success.

`devbox run ci` currently runs:

- `web-ui`: `pnpm run check`, `pnpm test`, and `pnpm run build`
- `cargo fmt --all --check`
- `cargo check --workspace --target wasm32-unknown-unknown`
- `cargo test --workspace`
- `WRANGLER_LOG=error wrangler deploy --dry-run`

Use `devbox run ci` as the minimum gate before sending a change.

## Model Checking
<!-- constrained-by ../../crates/cfwdon-models/src/quote.rs -->

Formal models for domain protocols live in [`crates/cfwdon-models`](../../crates/cfwdon-models). The crate uses [Stateright](https://www.stateright.rs/) to explore finite state spaces and check safety (`always`) and reachability (`sometimes`) properties.

The first model covers quote approval policy and quote-state resolution in [`crates/cfwdon-domain/src/quote.rs`](../../crates/cfwdon-domain/src/quote.rs). Transition steps delegate to the same pure helpers used in production, so the checker does not duplicate business rules.

A second model covers outbound delivery retries, inbox `Accept`/`Reject` responses, and remote-follow reconciliation in [`crates/cfwdon-domain/src/delivery.rs`](../../crates/cfwdon-domain/src/delivery.rs), including the rule that terminal failure of a `Follow` activity moves a pending follow to `failed` while inbox `Reject` moves it to `rejected`.

A third model checks that two concurrently processed outbound deliveries evolve independently while each slot still follows the same retry rules as production `buffer_unordered` processing.

A fourth model covers quote approval lifecycle transitions in [`crates/cfwdon-domain/src/quote.rs`](../../crates/cfwdon-domain/src/quote.rs): local and remote publish, federation re-upsert with sticky `revoked`, and owner approve/reject/revoke paths.

A fifth model covers local follow requests and inbound remote follow requests in [`crates/cfwdon-domain/src/follow.rs`](../../crates/cfwdon-domain/src/follow.rs), including locked-account `pending` queues and authorize/reject transitions.

A sixth model covers generic outbox expansion and target delivery in [`crates/cfwdon-domain/src/delivery.rs`](../../crates/cfwdon-domain/src/delivery.rs): generic rows complete without targets or expand into per-inbox target deliveries with the same retry rules.

A seventh model covers ActivityPub audience to visibility mapping in [`crates/cfwdon-domain/src/remote/activitypub.rs`](../../crates/cfwdon-domain/src/remote/activitypub.rs), including precedence of `to` over `cc` and quote-policy restrictions on private visibility.

An eighth model covers local status composition in [`crates/cfwdon-domain/src/status/draft.rs`](../../crates/cfwdon-domain/src/status/draft.rs): `ComposingStatus` validation, quote/media/poll constraints, and `PublishIntent` quote-policy and quote-state resolution.

A ninth model covers account registration in [`crates/cfwdon-domain/src/account/registration.rs`](../../crates/cfwdon-domain/src/account/registration.rs): field validation, `RegistrationIntent` assignment, and `LocalAccount` provisioning defaults.

A tenth model covers OAuth-style access provisioning in the same module: email resolution, username derivation with collision suffixing, and provisioning defaults for first-time authenticated users.

An eleventh model covers shared inbox replay dedupe in [`crates/cfwdon-domain/src/federation/inbox.rs`](../../crates/cfwdon-domain/src/federation/inbox.rs): first delivery is accepted, in-flight and processed replays are rejected, and failed processing releases the slot.

A twelfth model covers federation request policy in [`crates/cfwdon-domain/src/federation/`](../../crates/cfwdon-domain/src/federation/): ActivityPub signed-header requirements, actor keyId matching, static remote URL host policy, and request date skew bounds.

A sixteenth model covers DNS rebinding SSRF defense in [`crates/cfwdon-domain/src/federation/dns.rs`](../../crates/cfwdon-domain/src/federation/dns.rs): hostname resolution must return only public A/AAAA addresses after static host policy passes, matching the async DNS guard in the worker.

A thirteenth model covers eight-slot outbox delivery pools in [`crates/cfwdon-domain/src/delivery.rs`](../../crates/cfwdon-domain/src/delivery.rs), matching production `buffer_unordered(8)` concurrency while checking that each slot still follows the same retry rules independently.

Quote owner approve, reject, and revoke API handlers in the worker now delegate quote-state transitions to [`OwnerQuoteAction`](../../crates/cfwdon-domain/src/quote.rs) and remote status upserts merge quote state through `merged_quote_state_for_remote_upsert`. Approve and reject also emit FEP-044f `Accept`/`Reject` QuoteRequest activities to remote quote authors, fan out `Create QuoteAuthorization` to followers, and serve dereferenceable authorization stamps under `/users/:username/statuses/:id/quote_authorizations/:key`.

Registration transitions now emit typed [`RegistrationEvent`](../../crates/cfwdon-domain/src/account/registration.rs) values through [`Transition`](../../crates/cfwdon-domain/src/transition.rs), and a fourteenth model checks that validate and provision steps surface the expected events.

Status draft transitions now emit typed [`StatusDraftEvent`](../../crates/cfwdon-domain/src/status/draft.rs) values on validate and publish-intent resolution, and a fifteenth model checks those events are surfaced consistently.

Refinement mappings in [`docs/reference/model-refinement.md`](../reference/model-refinement.md) link each model to domain symbols and worker call sites. Executable refinement checks cover quote policy/state resolution, status draft publish, registration, federation, delivery, follow, and ActivityPub visibility inside `verify_models()`.

```sh
cargo test -p cfwdon-models
```

`devbox run test` and `devbox run ci` already run `cargo test --workspace`, so model checks are part of the normal gate.

## Local Worker
<!-- constrained-by ../reference/configuration.md -->

```sh
devbox run worker:dev
```

This starts `wrangler dev`, rebuilds `web-ui/dist`, stages UI files into `assets/`, and applies pending local D1 migrations before boot. The Worker build copies `web-ui/dist` to `assets/app` and `admin-ui/dist` to `assets/admin`, or fallback HTML when a dist directory is missing.

### Connect to a specific instance

```sh
# Local worker code + local D1, but Mastodon instance metadata uses this domain.
devbox run worker:dev -- --instance fedi.manji.app

# Local worker code against remote Cloudflare bindings (D1/KV/R2) for that deployment.
devbox run worker:dev -- --instance fedi.manji.app --remote
```

`--instance` accepts a bare domain (`fedi.manji.app`) or full URL (`https://fedi.manji.app`).
It overrides `INSTANCE_DOMAIN` and the matching Auth0 audience/email-claim vars for the dev process.

### Web UI hot reload against an instance

```sh
# Proxy API/auth routes to the local worker on :8787 (run worker:dev in another terminal).
devbox run web-ui:dev

# Proxy API/auth routes to a remote instance (read-only/public flows; Auth0 callback stays remote).
devbox run web-ui:dev -- --instance https://fedi.manji.app
```

`web-ui:dev` runs Vite on port `5173` with `/app/` hot reload. API routes under `/api`, `/oauth`, and `/app/login` are proxied to the configured origin.

### Auth0 on localhost

Local login sends Auth0 a callback URL such as `http://127.0.0.1:8787/oauth/auth0/callback`.
Add the following to the Auth0 application (same app as production, or a separate dev app via `.dev.vars`):

| Auth0 setting | Local values |
| --- | --- |
| Allowed Callback URLs | `http://127.0.0.1:8787/oauth/auth0/callback`, `http://localhost:8787/oauth/auth0/callback` |
| Allowed Logout URLs | `http://127.0.0.1:8787`, `http://localhost:8787` |
| Allowed Web Origins / CORS | same as logout URLs |

When using `web-ui:dev` against the local worker, also allow port `5173` with the same paths.

`devbox run worker:dev` prints this checklist on startup. See also [Auth0 Configuration Guide](../operations/auth0-configuration.md#local-development).

Routes that require D1, R2, or secrets need local or remote bindings configured according to the [Configuration Reference](../reference/configuration.md) and [Cloudflare Deploy Checklist](../operations/cloudflare-deploy.md).

## Mastodon API Compatibility Docs
<!-- derived-from ../mastodon-api-compat/README.md -->

Regenerate the compatibility inventory with:

```sh
python3 scripts/generate_mastodon_api_compat.py
```

Generated files live under `docs/mastodon-api-compat/`. Regenerate them whenever routes are added, removed, or intentionally diverge from Mastodon behavior, then review:

- `docs/mastodon-api-compat/README.md`
- `docs/mastodon-api-compat/inventory.md`
- `docs/mastodon-api-compat/todo-unimplemented.md`
- `docs/mastodon-api-compat/todo-compat.md`

## Git Notes

Use conventional commit messages, for example:

```sh
git commit -m "docs: update project documentation"
```

Before pushing, check the final diff:

```sh
git status --short
git diff --stat HEAD
```
