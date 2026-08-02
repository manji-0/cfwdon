#!/usr/bin/env bash
set -euo pipefail

PATH="/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:${PATH:-}"

BASE="${BASE_URL:-https://fedi.manji.app}"
ACCESS_TOKEN="${ACCESS_TOKEN:-}"

curl_http_code() {
  local tmp="$1"
  shift
  local code
  set +e
  code="$(curl -sS -o "$tmp" -w '%{http_code}' "$@")"
  local curl_status=$?
  set -e
  if [[ "$curl_status" -ne 0 && "$curl_status" -ne 28 ]]; then
    echo "FAIL curl_exit=${curl_status} $*" >&2
    return "$curl_status"
  fi
  printf '%s' "$code"
}

check_status() {
  local expected="$1"
  local path="$2"
  local auth="${3:-no}"
  local max_time="${4:-}"
  local tmp
  tmp="$(mktemp)"

  local curl_args=("${BASE}${path}")
  if [[ -n "$max_time" ]]; then
    curl_args=(--max-time "$max_time" "${curl_args[@]}")
  fi
  if [[ "$auth" == "yes" && -n "$ACCESS_TOKEN" ]]; then
    curl_args=(-H "Authorization: Bearer ${ACCESS_TOKEN}" "${curl_args[@]}")
  fi

  local code
  code="$(curl_http_code "$tmp" "${curl_args[@]}")" || {
    rm -f "$tmp"
    return 1
  }
  if [[ "$code" == "$expected" ]]; then
    echo "ok ${code} ${path}"
  else
    echo "FAIL expected=${expected} got=${code} ${path}" >&2
    if [[ -s "$tmp" ]]; then
      head -c 200 "$tmp" >&2
      echo >&2
    fi
    rm -f "$tmp"
    return 1
  fi
  rm -f "$tmp"
}

check_json_array() {
  local path="$1"
  local auth="${2:-no}"
  local tmp
  tmp="$(mktemp)"

  local curl_args=("${BASE}${path}")
  if [[ "$auth" == "yes" && -n "$ACCESS_TOKEN" ]]; then
    curl_args=(-H "Authorization: Bearer ${ACCESS_TOKEN}" "${curl_args[@]}")
  fi

  local code
  code="$(curl_http_code "$tmp" "${curl_args[@]}")" || {
    rm -f "$tmp"
    return 1
  }
  if [[ "$code" != "200" ]]; then
    echo "FAIL expected=200 got=${code} ${path}" >&2
    rm -f "$tmp"
    return 1
  fi

  python3 - "$tmp" "$path" <<'PY'
import json, sys
path = sys.argv[2]
with open(sys.argv[1], encoding="utf-8") as fh:
    data = json.load(fh)
if not isinstance(data, list):
    raise SystemExit(f"FAIL {path}: expected JSON array, got {type(data).__name__}")
print(f"ok 200 {path} (items={len(data)})")
PY
  rm -f "$tmp"
}

check_account_lookup() {
  local acct="${1:-manji0@fedi.manji.app}"
  local path="/api/v1/accounts/lookup?acct=${acct}"
  local tmp
  tmp="$(mktemp)"

  local code
  code="$(curl_http_code "$tmp" "${BASE}${path}")" || {
    rm -f "$tmp"
    return 1
  }
  if [[ "$code" != "200" ]]; then
    echo "FAIL expected=200 got=${code} ${path}" >&2
    rm -f "$tmp"
    return 1
  fi

  python3 - "$tmp" "$path" <<'PY'
import json, sys
path = sys.argv[2]
with open(sys.argv[1], encoding="utf-8") as fh:
    data = json.load(fh)
acct = data.get("acct")
acct_id = data.get("id")
if not acct or not acct_id:
    raise SystemExit(f"FAIL {path}: missing acct/id in response")
print(f"ok 200 {path} (acct={acct}, id={acct_id})")
PY
  rm -f "$tmp"
}

echo "base_url=${BASE}"
if [[ -n "$ACCESS_TOKEN" ]]; then
  echo "access_token=set"
else
  echo "access_token=unset (auth-required routes expect 401/422)"
fi
echo

# Public read endpoints
check_status 200 /api/v1/instance/activity
check_status 200 /api/v1/instance/peers
check_status 200 /api/v1/instance/rules
check_status 200 '/api/v1/trends/tags?limit=1'
check_status 200 '/api/v1/trends/statuses?limit=1'
check_status 200 /api/v1/streaming/public no 3

# Auth-required or parameterless routes (expected without token)
if [[ -n "$ACCESS_TOKEN" ]]; then
  check_status 200 /api/v1/preferences yes
  check_json_array /api/v1/announcements yes
  check_status 200 '/api/v1/accounts/search?q=manji&limit=5&resolve=false' yes
  check_status 200 /api/v1/notifications/policy yes
else
  check_status 401 /api/v1/preferences
  check_status 422 /api/v1/announcements
  check_status 401 '/api/v1/accounts/search?q=manji&limit=5&resolve=false'
  check_status 401 /api/v1/notifications/policy
fi

# OAuth authorize without params should reject the request
check_status 400 /oauth/authorize

# Public account lookup (replaces authenticated search smoke check)
check_account_lookup "manji0@fedi.manji.app"

echo
echo "all checks passed"
