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

`devbox run ci` currently runs:

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

```sh
cargo test -p cfwdon-models
```

`devbox run test` and `devbox run ci` already run `cargo test --workspace`, so model checks are part of the normal gate.

## Local Worker
<!-- constrained-by ../reference/configuration.md -->

```sh
devbox run worker:dev
```

This starts `wrangler dev`. Routes that require D1, R2, or secrets need local or remote bindings configured according to the [Configuration Reference](../reference/configuration.md) and [Cloudflare Deploy Checklist](../operations/cloudflare-deploy.md).

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
