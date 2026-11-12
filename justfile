# Task runner for cfwdon. Run inside the Nix dev shell:
#   nix develop -c just <recipe>
# or enter it once with `nix develop` (or direnv `use flake`).

default:
    @just --list

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

check:
    cargo check --workspace --target wasm32-unknown-unknown

clippy:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --target wasm32-unknown-unknown -- -D warnings

test:
    cargo test --workspace

ci-web-ui:
    cd web-ui && pnpm install --frozen-lockfile && pnpm run check && pnpm test && pnpm run build

ci-server: fmt-check clippy test
    python3 scripts/check_migrations.py
    python3 scripts/check_query_plans.py
    python3 scripts/check_module_layers.py
    python3 scripts/check_pending_account_guard.py
    python3 scripts/check_pending_account_guard.py --self-test

ci-wrangler-dry-run:
    WRANGLER_LOG=error wrangler deploy --dry-run

ci: ci-web-ui ci-server ci-wrangler-dry-run

worker-dev *args:
    node scripts/run_worker_dev.mjs {{args}}

web-ui-dev *args:
    node scripts/run_web_ui_dev.mjs {{args}}

# Approve a pending account by username or e-mail, e.g. the first admin:
#   just approve-account --remote alice
approve-account *args:
    bash scripts/approve_account.sh {{args}}

# D1 statements and serial rounds per API request on a seeded local DB
# (build the worker first); see scripts/query_count/run.sh.
query-count *args:
    bash scripts/query_count/run.sh {{args}}
