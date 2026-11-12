#!/usr/bin/env bash
# Count the D1 statements and serial D1 rounds per Mastodon API request on a
# seeded local database. Build the worker first (`WORKER_BUILD_PROFILE=dev
# node scripts/build_worker.mjs`), then:
#   bash scripts/query_count/run.sh            # the table
#   bash scripts/query_count/run.sh "POST"     # plus the SQL of matching rows
#   SHAPE=1 bash scripts/query_count/run.sh    # plus normalized response bodies
#   NOISE=20000 bash scripts/query_count/run.sh  # first add unrelated rows (noise.sql)
# Rows: QUERY_COUNT_ROW|label|status|items|statements|rounds|batches|after|
#   rows_read|rows_written|do_requests|queue_messages|kv_reads|kv_writes|workflows
# (statements and rounds before the response; `after` ran once it was sent;
# the billed columns are totals, deferred work included).
# tstodon prints the same rows from its query-count.bench.test.ts.
set -euo pipefail
repo="$(cd "$(dirname "$0")/../.." && pwd)"
here="$repo/scripts/query_count"
state="$(mktemp -d)"
port="${PORT:-8787}"
config="$repo/wrangler.query-count.toml"
sed -e 's#^main = .*#main = "crates/cfwdon-worker/build/query-count-entry.mjs"#' \
  -e '/^\[build\]/,/^command/d' \
  -e "s#fedi.manji.app\"\$#localhost:$port\"#" \
  "$repo/wrangler.toml" > "$config"
cp "$here/entry.mjs" "$repo/crates/cfwdon-worker/build/query-count-entry.mjs"
cd "$repo"
wrangler d1 migrations apply DB --local -c "$config" --persist-to "$state" > /dev/null
wrangler d1 execute DB --local -c "$config" --persist-to "$state" --file "$here/seed.sql" > /dev/null
if [ "${NOISE:-0}" -gt 0 ]; then
  sed "s/@N@/$NOISE/g" "$here/noise.sql" > "$state/noise.sql"
  wrangler d1 execute DB --local -c "$config" --persist-to "$state" --file "$state/noise.sql" > /dev/null
fi
setsid wrangler dev -c "$config" --persist-to "$state" --port "$port" > "$state/dev.log" 2>&1 &
pgid=$!
trap 'kill -- -"$pgid" 2>/dev/null || true; rm -f "$config"' EXIT
for _ in $(seq 1 90); do
  curl -sf "localhost:$port/__qc" > /dev/null && break
  sleep 1
done
BASE="http://localhost:$port" VERBOSE="${1:-}" node "$here/scenario.mjs"
