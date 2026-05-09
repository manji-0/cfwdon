#!/usr/bin/env bash
set -euo pipefail

out_dir=${1:-dist/phanpy-webui}
phanpy_ref=${PHANPY_REF:-production}
work_dir=${PHANPY_WORK_DIR:-}

cleanup() {
  if [[ -n "${tmp_dir:-}" && -d "$tmp_dir" ]]; then
    rm -rf "$tmp_dir"
  fi
}
trap cleanup EXIT

if [[ -z "$work_dir" ]]; then
  tmp_dir=$(mktemp -d)
  work_dir="$tmp_dir/phanpy"
  git clone --depth 1 --branch "$phanpy_ref" https://github.com/cheeaun/phanpy.git "$work_dir"
fi

pushd "$work_dir" >/dev/null
npm install
env_args=(
  "PHANPY_CLIENT_NAME=${PHANPY_CLIENT_NAME:-cfwdon}"
  "PHANPY_WEBSITE=${PHANPY_WEBSITE:-https://fedi.manji.app}"
  "PHANPY_DEFAULT_INSTANCE=${PHANPY_DEFAULT_INSTANCE:-fedi.manji.app}"
  "PHANPY_DEFAULT_LANG=${PHANPY_DEFAULT_LANG:-ja}"
  "PHANPY_REFERRER_POLICY=${PHANPY_REFERRER_POLICY:-origin}"
  "PHANPY_DISALLOW_ROBOTS=${PHANPY_DISALLOW_ROBOTS:-true}"
)
if [[ -n "${PHANPY_DEFAULT_INSTANCE_REGISTRATION_URL:-}" ]]; then
  env_args+=("PHANPY_DEFAULT_INSTANCE_REGISTRATION_URL=$PHANPY_DEFAULT_INSTANCE_REGISTRATION_URL")
fi
if [[ -n "${PHANPY_PRIVACY_POLICY_URL:-}" ]]; then
  env_args+=("PHANPY_PRIVACY_POLICY_URL=$PHANPY_PRIVACY_POLICY_URL")
fi
env "${env_args[@]}" npm run build
popd >/dev/null

rm -rf "$out_dir"
mkdir -p "$out_dir"
cp -R "$work_dir/dist/." "$out_dir/"

printf 'Phanpy build copied to %s\n' "$out_dir"
