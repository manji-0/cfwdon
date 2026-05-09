# Development Workflow

`cfwdon` は Rust workspace と Cloudflare Worker tooling を `devbox` で固定する。通常の開発では repo root からコマンドを実行する。

## Prerequisites

- `devbox`
- Cloudflare Workers / D1 / R2 を操作する場合は `wrangler` の認証
- VCS はこの workspace では `jj` を使う

`devbox.json` は次を用意する。

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

`devbox run ci` は現在、次を実行する。

- `cargo fmt --all --check`
- `cargo check --workspace --target wasm32-unknown-unknown`
- `cargo test --workspace`
- `WRANGLER_LOG=error wrangler deploy --dry-run`

Cloudflare Worker として壊れていないことを確認するには、少なくとも `devbox run ci` を通す。

## Local Worker
<!-- constrained-by ../reference/configuration.md -->

```sh
devbox run worker:dev
```

このコマンドは `wrangler dev` を起動する。D1 / R2 / secrets が必要な route を確認する場合は、[Configuration Reference](../reference/configuration.md) と [Cloudflare Deploy Checklist](../operations/cloudflare-deploy.md) に沿ってローカルまたは remote の binding を準備する。

## Mastodon API Compatibility Docs
<!-- derived-from ../mastodon-api-compat/README.md -->

互換性 inventory は script で再生成する。

```sh
python3 scripts/generate_mastodon_api_compat.py
```

生成対象は `docs/mastodon-api-compat/` 配下の Markdown である。route を追加、削除、互換性メモを変更した場合は再生成し、次を確認する。

- `docs/mastodon-api-compat/README.md` の snapshot
- `docs/mastodon-api-compat/inventory.md` の route mapping
- `docs/mastodon-api-compat/todo-unimplemented.md`
- `docs/mastodon-api-compat/todo-compat.md`

## VCS Notes

この repo の作業履歴は `jj` で扱う。

```sh
jj status
```

```sh
jj diff
```

```sh
jj describe -m "docs: update project documentation"
```

```sh
jj bookmark set main -r @
jj git push -b main
```

作業前に remote 状態を取り込む場合は次を使う。

```sh
jj git fetch
```
