#!/usr/bin/env bash
set -euo pipefail

PATH="/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:${PATH:-}"

RUN_ID="${RUN_ID:-$(openssl rand -hex 6)}"
UA="cfwdon-read-bench/${RUN_ID}"
BASE="${BASE_URL:-https://fedi.manji.app}"
ITERATIONS="${ITERATIONS:-3}"

STATUS_ID="$(curl -sS -A "$UA" "${BASE}/api/v1/timelines/public?local=true&limit=1" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d[0]["id"])')"

bench() {
  local name="$1"
  local session="$2"
  local path="$3"
  local cold warm_line bookmark=0

  cold="$(curl -sS -o /dev/null -w '%{http_code} %{time_total}' -A "$UA" "${BASE}${path}")"
  warm_line=""
  for _ in $(seq 1 "$ITERATIONS"); do
    local hdr
    hdr="$(mktemp)"
    local sample
    sample="$(curl -sS -D "$hdr" -o /dev/null -w '%{http_code} %{time_total}' -A "$UA" "${BASE}${path}")"
    if grep -qi '^x-d1-bookmark:' "$hdr"; then
      bookmark=$((bookmark + 1))
    fi
    warm_line+="${sample} "
    rm -f "$hdr"
  done
  printf '%s\n' "${name}|${session}|${path}|${cold}|${warm_line}|bookmark=${bookmark}/${ITERATIONS}"
}

echo "run_id=${RUN_ID}"
echo "user_agent=${UA}"
echo "status_id=${STATUS_ID}"
echo "base_url=${BASE}"
echo

bench instance_v1 no /api/v1/instance
bench instance_v2 no /api/v2/instance
bench instance_activity no /api/v1/instance/activity
bench public_timeline yes '/api/v1/timelines/public?limit=20'
bench public_local yes '/api/v1/timelines/public?local=true&limit=20'
bench custom_emojis no /api/v1/custom_emojis
bench trends_tags no '/api/v1/trends/tags?limit=10'
bench trends_statuses no '/api/v1/trends/statuses?limit=10'
bench account_lookup no '/api/v1/accounts/lookup?acct=manji0@fedi.manji.app'
bench status_show yes "/api/v1/statuses/${STATUS_ID}"
bench status_context yes "/api/v1/statuses/${STATUS_ID}/context"
bench nodeinfo no /.well-known/nodeinfo
