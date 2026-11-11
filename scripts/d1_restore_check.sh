#!/usr/bin/env bash
# Restore drill for a scheduled D1 backup, entirely local: decrypt the backup,
# load it into a scratch local D1 database (Miniflare's SQLite, under a
# temporary directory), and report what it holds and which of the repository's
# migrations it lacks. Nothing touches Cloudflare.
#
#   scripts/d1_restore_check.sh <backup.sql.gz.age> <age-identity-file> [--keep]
#
# Needs `age` and `wrangler` on PATH (the Nix dev shell has wrangler). --keep
# leaves the decrypted dump and the scratch database in place (the path is
# printed) instead of deleting them. The dump holds account private keys and
# private posts: keep it only on an encrypted disk. See
# docs/operations/runbook.md#restore-from-a-scheduled-backup.
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <backup.sql.gz.age> <age-identity-file> [--keep]" >&2
  exit 2
fi
backup="$(realpath "$1")"
identity="$(realpath "$2")"
keep="${3:-}"
repo="$(cd "$(dirname "$0")/.." && pwd)"
command -v wrangler > /dev/null || { echo "wrangler not on PATH (use nix develop)" >&2; exit 1; }
command -v age > /dev/null || { echo "age not on PATH" >&2; exit 1; }

umask 077
work="$(mktemp -d "${TMPDIR:-/tmp}/cfwdon-restore-check.XXXXXX")"
cleanup() {
  if [ "$keep" = "--keep" ]; then
    echo "kept: $work"
  else
    rm -rf "$work"
  fi
}
trap cleanup EXIT

if [ -f "$backup.sha256" ]; then
  [ "$(sha256sum "$backup" | cut -d' ' -f1)" = "$(cat "$backup.sha256")" ] \
    || { echo "sha256 does not match $backup.sha256" >&2; exit 1; }
  echo "sha256 matches"
fi

age --decrypt -i "$identity" "$backup" | gunzip > "$work/restore.sql"
echo "decrypted: $(wc -c < "$work/restore.sql") bytes"

printf '{"name":"cfwdon-restore-check","compatibility_date":"2026-04-13","d1_databases":[{"binding":"DB","database_name":"cfwdon-restore-check","database_id":"00000000-0000-0000-0000-000000000000","migrations_dir":"%s"}]}' \
  "$repo/migrations" > "$work/wrangler.json"

cd "$work"
export CI=1 WRANGLER_SEND_METRICS=false
wrangler d1 execute cfwdon-restore-check --local --file "$work/restore.sql" > "$work/import.log" 2>&1 \
  || { cat "$work/import.log" >&2; exit 1; }
echo "imported into a scratch local database"

wrangler d1 execute cfwdon-restore-check --local --json --command \
  "SELECT (SELECT COUNT(*) FROM accounts) AS accounts,
          (SELECT COUNT(*) FROM statuses) AS statuses,
          (SELECT COUNT(*) FROM outbox_deliveries) AS outbox_deliveries,
          (SELECT MAX(name) FROM d1_migrations) AS last_migration" 2> /dev/null \
  | node -e '
      let text = "";
      process.stdin.on("data", (chunk) => (text += chunk));
      process.stdin.on("end", () => console.log(JSON.parse(text)[0].results[0]));
    '

# Migrations newer than the backup are what `wrangler d1 migrations apply`
# would run after a restore; a listed one older than the backup means the
# backup came from a different branch.
wrangler d1 migrations list cfwdon-restore-check --local 2>&1 | sed -n '/igrations/,$p'
