# cfwdon

`cfwdon` is a Mastodon-compatible server for Cloudflare Workers. It is written in Rust and uses Cloudflare D1 for relational state, R2 for media storage, and Auth0 as the authentication boundary for protected API routes.

The project is still early software. The current focus is making the Mastodon API surface, ActivityPub federation behavior, and Cloudflare deployment story explicit enough that contributors can work on compatibility without first reverse-engineering the whole Worker.

## Status

- Rust workspace with `cfwdon-core`, `cfwdon-domain`, and `cfwdon-worker`
- Cloudflare Worker deployment through `wrangler`
- D1 migrations and R2 media bindings
- Mastodon API compatibility inventory generated from upstream routes
- ActivityPub actor, inbox, outbox, delivery, follow, status, poll, and interaction slices

For the detailed route inventory, see [Mastodon API Compatibility](docs/mastodon-api-compat/README.md).

## Requirements

- `devbox`
- Cloudflare account access for Workers, D1, and R2 operations
- `wrangler` authentication for deploys and remote resource changes

The development shell installs the Rust toolchain, `wasm32-unknown-unknown`, `wrangler`, `worker-build`, `wasm-bindgen-cli`, `binaryen`, and supporting tools declared in [devbox.json](devbox.json).

## Quick Start
<!-- derived-from ./docs/getting-started/clone-and-run.md -->

```sh
devbox shell
devbox run ci
```

To run the Worker locally:

```sh
devbox run worker:dev
```

Local routes that depend on D1, R2, or Auth0 need matching local or remote bindings. Start with [Clone And Run](docs/getting-started/clone-and-run.md), then use [Development Workflow](docs/getting-started/development.md) for the day-to-day loop and [Configuration Reference](docs/reference/configuration.md) for runtime values.

## Documentation
<!-- derived-from ./docs/README.md -->

- [Documentation Index](docs/README.md)
- [Clone And Run](docs/getting-started/clone-and-run.md)
- [Development Workflow](docs/getting-started/development.md)
- [Configuration Reference](docs/reference/configuration.md)
- [Cloudflare Deploy Checklist](docs/operations/cloudflare-deploy.md)
- [Architecture](docs/architecture/cfwdon-architecture.md)
- [Project TODO](docs/planning/full-todo.md)
- [Mastodon API Compatibility](docs/mastodon-api-compat/README.md)

## Compatibility Docs
<!-- derived-from ./docs/mastodon-api-compat/README.md -->

The Mastodon API compatibility files are generated from upstream Mastodon routes and the local Worker router:

```sh
python3 scripts/generate_mastodon_api_compat.py
```

Regenerate them when route handlers are added, removed, or intentionally diverge from Mastodon behavior.

## Contributing
<!-- constrained-by ./docs/getting-started/development.md -->

Before opening a change, run:

```sh
devbox run ci
```

Use conventional commit messages for commits. For documentation changes, keep hand-written docs under `docs/getting-started`, `docs/reference`, `docs/operations`, `docs/architecture`, or `docs/planning`; keep generated compatibility output under `docs/mastodon-api-compat`.

## License

AGPL-3.0-or-later.
