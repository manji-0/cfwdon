#!/usr/bin/env bash
# Cloud Agent install phase for cfwdon.
#
# Prepares the devbox/Nix toolchain and warms Rust build caches so that the
# canonical commands documented in AGENTS.md work out of the box:
#   devbox run ci          # fmt + wasm check + clippy + tests + wrangler dry-run
#   devbox run worker:dev  # wrangler dev
#
# The script is idempotent. On a prebuilt environment it finds Nix, devbox, and
# warm caches already in place and simply reconciles them; on a bare image it
# bootstraps everything from scratch.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

log() { printf '\n[cfwdon-install] %s\n' "$*"; }

# 1. Ensure the devbox binary is present.
if ! command -v devbox >/dev/null 2>&1; then
  log "Installing devbox..."
  curl -fsSL https://get.jetify.com/devbox -o /tmp/install-devbox.sh
  FORCE=1 bash /tmp/install-devbox.sh -f
fi

# 2. Ensure Nix is installed. devbox's first run installs it, but because this
#    VM has no systemd the daemon self-test fails and package sync errors out,
#    so tolerate a non-zero exit on this bootstrap attempt.
if [ ! -x /nix/var/nix/profiles/default/bin/nix-daemon ]; then
  log "Bootstrapping Nix via devbox (daemon self-test failure is expected here)..."
  yes | devbox install || true
fi

# 3. Ensure the nix-daemon is running before any further devbox commands.
bash "$REPO_ROOT/.cursor/start.sh"

# 4. Sync the pinned devbox packages (nodejs, wrangler, worker-build, binaryen,
#    wasm-bindgen-cli, etc.). Safe to re-run.
log "Syncing devbox packages..."
devbox install

# 5. Warm the Rust toolchain (the devbox init hook installs the pinned rustup
#    toolchain + wasm32 target on first use) and prime build caches for both the
#    native and wasm targets so the first CI / dev-server run is fast.
log "Warming Rust toolchain and build caches..."
devbox run -- sh -c 'cargo --version && rustc --version && wrangler --version && worker-build --version'
devbox run -- cargo fetch --locked
devbox run check
devbox run -- cargo build --workspace --all-targets

log "Install complete. Run 'devbox run ci' or 'devbox run worker:dev' to get started."
