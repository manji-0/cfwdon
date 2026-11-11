#!/usr/bin/env bash
# Approve a pending account from the command line, as POST
# /api/v1/admin/accounts/:id/approve does: delete its pending_account_approvals
# row. Use it to approve the first admin, who has no approved account to sign
# in to /admin with yet. See docs/operations/launch-checklist.md#the-first-admin.
#
#   scripts/approve_account.sh --local|--remote [--database NAME] <username|e-mail>
#
# --local edits the `wrangler dev` database, --remote the deployed one.
# --database defaults to the DB binding. Needs wrangler and node on PATH (the
# Nix dev shell has both). Run it from the repository root, where wrangler.toml
# is.
set -euo pipefail

usage() {
  echo "usage: $0 --local|--remote [--database NAME] <username|e-mail>" >&2
  exit 2
}

target=""
database="DB"
who=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --local | --remote)
      [ -z "$target" ] || usage
      target="$1"
      ;;
    --database)
      [ "$#" -ge 2 ] || usage
      database="$2"
      shift
      ;;
    -*) usage ;;
    *)
      [ -z "$who" ] || usage
      who="$1"
      ;;
  esac
  shift
done
[ -n "$target" ] && [ -n "$who" ] || usage

# wrangler d1 execute takes no bind parameters, so the value goes into the SQL
# text: accept only characters a username or an address can hold, none of
# which can end the quoted literal.
if [[ "$who" == *@* ]]; then
  [[ "$who" =~ ^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+$ ]] || {
    echo "not an e-mail address: $who" >&2
    exit 2
  }
  match="lower(access_email) = lower('$who')"
else
  [[ "$who" =~ ^[A-Za-z0-9_]{1,30}$ ]] || {
    echo "not a local username: $who" >&2
    exit 2
  }
  match="username = lower('$who')"
fi
[[ "$database" =~ ^[A-Za-z0-9_-]+$ ]] || {
  echo "not a database name: $database" >&2
  exit 2
}
command -v wrangler > /dev/null || { echo "wrangler not on PATH (use nix develop)" >&2; exit 1; }

# Prints the rows of the last statement's results, one JSON object per line.
d1() {
  wrangler d1 execute "$database" "$target" --json --command "$1" \
    | node -e '
      let input = "";
      process.stdin.on("data", (chunk) => (input += chunk));
      process.stdin.on("end", () => {
        const results = JSON.parse(input);
        for (const row of results.at(-1)?.results ?? []) console.log(JSON.stringify(row));
      });
    '
}

account="$(d1 "SELECT a.id, a.username, a.access_email,
                      EXISTS (SELECT 1 FROM pending_account_approvals p
                              WHERE p.account_id = a.id) AS pending
               FROM accounts a WHERE $match")"
if [ -z "$account" ]; then
  echo "no local account matches $who" >&2
  exit 1
fi
if [ "$(printf '%s\n' "$account" | wc -l)" -gt 1 ]; then
  echo "more than one account matches $who; approve by username:" >&2
  printf '%s\n' "$account" >&2
  exit 1
fi
echo "account: $account"

approved="$(d1 "DELETE FROM pending_account_approvals
                WHERE account_id = (SELECT id FROM accounts WHERE $match)
                RETURNING account_id")"
if [ -z "$approved" ]; then
  echo "already approved; nothing to do"
else
  echo "approved"
fi
