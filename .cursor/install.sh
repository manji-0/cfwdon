#!/usr/bin/env bash
# Cloud Agent install phase for cfwdon.
#
# Prepares the devbox/Nix toolchain and warms Rust build caches so that the
# canonical commands documented in AGENTS.md work out of the box:
#   devbox run ci          # fmt + wasm check + clippy + tests + wrangler dry-run
#   devbox run worker:dev  # wrangler dev
#   dagayn tool …          # FTS-only knowledge graph (no embeddings)
#
# The script is idempotent. On a prebuilt environment it finds Nix, devbox, and
# warm caches already in place and simply reconciles them; on a bare image it
# bootstraps everything from scratch.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

log() { printf '\n[cfwdon-install] %s\n' "$*"; }

# Pin the devbox CLI to the same version used in .github/workflows/ci.yml.
DEVBOX_VERSION="0.17.5"
# Pin dagayn so Cloud Agent graph analysis stays reproducible and embedding-free.
DAGAYN_VERSION="4.14.0"
DEVBOX_RELEASE_BASE="https://github.com/jetify-com/devbox/releases/download/${DEVBOX_VERSION}"

# SHA256 checksums from the official 0.17.5 release assets:
# https://github.com/jetify-com/devbox/releases/download/0.17.5/checksums.txt
declare -A DEVBOX_CHECKSUMS=(
  [linux_amd64]=eb2d8fb34266ba3befc294d7d6f56e2cd4da2cacb7a0cf52db5b8092575544f8
  [linux_arm64]=880901fff1ce7bf48086c12d84535bc14c257b56cb0d05e93e037f2cb1b1d529
  [darwin_amd64]=715480b386a4ed2a14c4eb766e9074772c36a00593d57c3dafc834da5c7fb60f
  [darwin_arm64]=0684fecd68bf2009a2ad57be1ba1ea2bbd735a02017fff355cea0f1b15a7e00f
)

install_devbox() {
  local os arch platform archive expected_checksum actual_checksum tmpdir installed_version

  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  case "$(uname -m)" in
    x86_64 | amd64) arch=amd64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *)
      log "ERROR: unsupported architecture: $(uname -m)"
      exit 1
      ;;
  esac

  platform="${os}_${arch}"
  expected_checksum="${DEVBOX_CHECKSUMS[$platform]:-}"
  if [ -z "$expected_checksum" ]; then
    log "ERROR: unsupported platform: ${platform}"
    exit 1
  fi

  archive="devbox_${DEVBOX_VERSION}_${platform}.tar.gz"
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' RETURN

  log "Downloading devbox ${DEVBOX_VERSION} for ${platform}..."
  curl -fsSL "${DEVBOX_RELEASE_BASE}/${archive}" -o "${tmpdir}/${archive}"

  actual_checksum="$(sha256sum "${tmpdir}/${archive}" | awk '{print $1}')"
  if [ "$actual_checksum" != "$expected_checksum" ]; then
    log "ERROR: devbox checksum mismatch (expected ${expected_checksum}, got ${actual_checksum})"
    exit 1
  fi

  tar -xzf "${tmpdir}/${archive}" -C "$tmpdir"
  mkdir -p "${HOME}/.local/bin"
  install -m 0755 "${tmpdir}/devbox" "${HOME}/.local/bin/devbox"
  export PATH="${HOME}/.local/bin:${PATH}"

  installed_version="$(devbox version)"
  if [ "$installed_version" != "$DEVBOX_VERSION" ]; then
    log "ERROR: devbox version mismatch (expected ${DEVBOX_VERSION}, got ${installed_version})"
    exit 1
  fi
}

install_dagayn() {
  mkdir -p "${HOME}/.local/bin"
  export PATH="${HOME}/.local/bin:${PATH}"

  local installed_version=""
  if command -v dagayn >/dev/null 2>&1; then
    installed_version="$(dagayn --version 2>/dev/null || true)"
  fi
  if echo "${installed_version}" | grep -Fq "${DAGAYN_VERSION}"; then
    log "dagayn ${DAGAYN_VERSION} already installed."
  else
    if [ -n "${installed_version}" ]; then
      log "dagayn '${installed_version}' on PATH does not match pinned ${DAGAYN_VERSION}; reinstalling..."
    else
      log "Installing dagayn ${DAGAYN_VERSION} (FTS-only, no embeddings)..."
    fi
    python3 -m pip install --user --break-system-packages "dagayn==${DAGAYN_VERSION}"
    hash -r 2>/dev/null || true
    installed_version="$(dagayn --version 2>/dev/null || true)"
    if ! echo "${installed_version}" | grep -Fq "${DAGAYN_VERSION}"; then
      log "ERROR: dagayn version mismatch (expected ${DAGAYN_VERSION}, got ${installed_version})"
      exit 1
    fi
  fi

  # Cloud Agents load MCP from the dashboard and/or user config; keep a
  # user-level stdio entry that never starts the embedding sidecar.
  mkdir -p "${HOME}/.cursor"
  cat > "${HOME}/.cursor/mcp.json" <<'EOF'
{
  "mcpServers": {
    "dagayn": {
      "command": "dagayn",
      "args": [
        "serve"
      ],
      "type": "stdio"
    }
  }
}
EOF
}

build_dagayn_graph() {
  export PATH="${HOME}/.local/bin:${PATH}"
  if [ -f "${REPO_ROOT}/.dagayn/graph.db" ]; then
    log "Refreshing dagayn graph (FTS-only, no embeddings)..."
    dagayn session prepare --repo "${REPO_ROOT}" --embedding skip --budget-seconds 0
  else
    log "Building dagayn graph (FTS-only, no embeddings)..."
    dagayn build --repo "${REPO_ROOT}"
  fi
  dagayn status --repo "${REPO_ROOT}"
}

# 1. Ensure the pinned devbox binary is first on PATH (not an older Jetify install).
if ! command -v devbox >/dev/null 2>&1; then
  log "Installing devbox ${DEVBOX_VERSION}..."
  install_devbox
elif [ "$(devbox version)" != "$DEVBOX_VERSION" ]; then
  log "devbox $(devbox version) on PATH does not match pinned ${DEVBOX_VERSION}; reinstalling..."
  install_devbox
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

# 6. Install dagayn and build/refresh the FTS-only knowledge graph. Embeddings
#    are intentionally skipped: Cloud Agents query via `dagayn serve` / `dagayn tool`.
install_dagayn
build_dagayn_graph

log "Install complete. Run 'devbox run ci' or 'devbox run worker:dev' to get started."
