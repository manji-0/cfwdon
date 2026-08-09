#!/usr/bin/env bash
# Cloud Agent start phase for cfwdon.
#
# cfwdon's toolchain is provisioned with devbox, which uses multi-user Nix.
# Multi-user Nix relies on the nix-daemon, but Cloud Agent VMs have no systemd
# to supervise it, so the daemon socket never appears on its own. Every
# `devbox run ...` invocation needs that socket, so we start the daemon here on
# each boot. This script is idempotent: it exits early when the socket already
# exists and never starts a second daemon.
set -euo pipefail

DAEMON=/nix/var/nix/profiles/default/bin/nix-daemon
SOCKET=/nix/var/nix/daemon-socket/socket

if [ ! -x "$DAEMON" ]; then
  echo "[cfwdon-start] Nix is not installed yet; nothing to start."
  exit 0
fi

if [ -S "$SOCKET" ]; then
  echo "[cfwdon-start] nix-daemon socket already present; nothing to do."
  exit 0
fi

echo "[cfwdon-start] Starting nix-daemon..."
# setsid detaches the daemon from this script's session so it survives after
# start returns; sudo is required because the daemon manages /nix as root.
sudo setsid "$DAEMON" >/var/tmp/nix-daemon.log 2>&1 &
disown || true

for _ in $(seq 1 40); do
  if [ -S "$SOCKET" ]; then
    echo "[cfwdon-start] nix-daemon is ready."
    exit 0
  fi
  sleep 0.5
done

echo "[cfwdon-start] WARNING: nix-daemon socket did not appear within 20s." >&2
echo "[cfwdon-start] See /var/tmp/nix-daemon.log for details." >&2
exit 0
