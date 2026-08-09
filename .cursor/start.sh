#!/usr/bin/env bash
# Cloud Agent start phase for cfwdon.
#
# cfwdon's toolchain is provisioned with devbox, which uses multi-user Nix.
# Multi-user Nix relies on the nix-daemon, but Cloud Agent VMs have no systemd
# to supervise it, so the daemon socket never comes up on its own. Every
# `devbox run ...` invocation needs a *live* daemon, so we start it here on each
# boot.
#
# Important: when a VM boots from a prebuilt environment snapshot, the daemon
# socket file lingers on disk from when the snapshot was taken, but no daemon
# process is actually listening on it. Checking only for the socket file would
# therefore falsely conclude the daemon is up. We instead check for a running
# nix-daemon process, remove any stale socket, and (re)start the daemon.
set -euo pipefail

DAEMON=/nix/var/nix/profiles/default/bin/nix-daemon
SOCKET=/nix/var/nix/daemon-socket/socket

if [ ! -x "$DAEMON" ]; then
  echo "[cfwdon-start] Nix is not installed yet; nothing to start."
  exit 0
fi

if pgrep -f nix-daemon >/dev/null 2>&1; then
  echo "[cfwdon-start] nix-daemon is already running; nothing to do."
  exit 0
fi

# No live daemon. Drop any stale socket left behind by a snapshot restore so the
# fresh daemon can bind cleanly.
sudo rm -f "$SOCKET" 2>/dev/null || true

echo "[cfwdon-start] Starting nix-daemon..."
# setsid detaches the daemon from this script's session so it survives after
# start returns; sudo is required because the daemon manages /nix as root.
sudo setsid "$DAEMON" >/var/tmp/nix-daemon.log 2>&1 &
disown || true

for _ in $(seq 1 40); do
  if [ -S "$SOCKET" ] && pgrep -f nix-daemon >/dev/null 2>&1; then
    echo "[cfwdon-start] nix-daemon is ready."
    exit 0
  fi
  sleep 0.5
done

echo "[cfwdon-start] WARNING: nix-daemon did not become ready within 20s." >&2
echo "[cfwdon-start] See /var/tmp/nix-daemon.log for details." >&2
exit 0
