#!/usr/bin/env bash
# Cloud Agent install phase for cfwdon.
#
# Prepares the Nix flake dev shell and warms Rust build caches so that the
# canonical commands documented in AGENTS.md work out of the box:
#   nix develop -c just ci          # fmt + clippy + tests + wrangler dry-run
#   nix develop -c just worker-dev  # wrangler dev
#
# The script is idempotent. On a prebuilt environment it finds Nix and warm
# caches already in place and simply reconciles them; on a bare image it
# bootstraps everything from scratch.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

log() { printf '\n[cfwdon-install] %s\n' "$*"; }

export NIX_CONFIG="experimental-features = nix-command flakes"

# 1. Ensure multi-user Nix is installed. The installer's daemon self-test fails
#    on VMs without systemd, so tolerate a non-zero exit and start the daemon
#    ourselves below.
if [ ! -x /nix/var/nix/profiles/default/bin/nix-daemon ]; then
  log "Installing Nix (daemon self-test failure is expected here)..."
  curl -fsSL https://nixos.org/nix/install | sh -s -- --daemon --yes || true
fi
export PATH="/nix/var/nix/profiles/default/bin:${PATH}"

# 2. Ensure the nix-daemon is running before any nix command.
bash "$REPO_ROOT/.cursor/start.sh"

# 3. Realise the dev shell (Rust toolchain, nodejs, wrangler, worker-build,
#    binaryen, wasm-bindgen-cli, just, ...) and prime build caches for both the
#    native and wasm targets so the first CI / dev-server run is fast.
log "Warming the Nix dev shell and Rust build caches..."
nix develop -c sh -c 'cargo --version && rustc --version && wrangler --version && worker-build --version'
nix develop -c cargo fetch --locked
nix develop -c just check
nix develop -c cargo build --workspace --all-targets

log "Install complete. Run 'nix develop -c just ci' or 'nix develop -c just worker-dev' to get started."
