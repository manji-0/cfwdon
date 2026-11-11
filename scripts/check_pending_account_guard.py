#!/usr/bin/env python3
"""Fail on SQL that resolves a credential to an account without its approval.

An account waiting for approval (a `pending_account_approvals` row) must not
act through its OAuth token or its e-mail. Every SQL string literal in the
worker that

- joins `oauth_access_tokens` to `accounts`, or
- reads `accounts` by `access_email`,

must name `pending_account_approvals` in the same literal, so the lookup
returns the account's approval with it. A lookup that must see every account
regardless of approval goes in ALLOWED with the reason.

Usage: check_pending_account_guard.py [--root REPO_ROOT]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

WORKER_SRC = Path("crates/cfwdon-worker/src")

# (path relative to WORKER_SRC, enclosing fn) -> why it may skip the approval.
ALLOWED = {
    ("auth/account_store.rs", "find_account_id_holding_email"): (
        "uniqueness and sign-in existence checks must see pending accounts; "
        "it returns only an id, never an account to act as"
    ),
}

STRING_RE = re.compile(r'r(#*)"(.*?)"\1|"((?:\\.|[^"\\])*)"', re.S)
FN_RE = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)")
TOKEN_JOIN_RE = re.compile(r"\boauth_access_tokens\b.*\bJOIN\s+accounts\b", re.S | re.I)
FROM_ACCOUNTS_RE = re.compile(r"\b(?:FROM|JOIN)\s+accounts\b", re.I)
BY_EMAIL_RE = re.compile(r"\baccess_email\s*=\s*\?", re.I)
SELECT_RE = re.compile(r"\bSELECT\b", re.I)


def test_code_start(text: str) -> int:
    """Offset of the first `#[cfg(test)]` module; test fixtures follow it."""
    match = re.search(r"(?m)^\s*#\[cfg\(test\)\]\s*\n\s*mod\s", text)
    return match.start() if match else len(text)


def unguarded(sql: str) -> str | None:
    if "pending_account_approvals" in sql:
        return None
    if TOKEN_JOIN_RE.search(sql):
        return "joins oauth_access_tokens to accounts"
    if SELECT_RE.search(sql) and FROM_ACCOUNTS_RE.search(sql) and BY_EMAIL_RE.search(sql):
        return "reads accounts by access_email"
    return None


def enclosing_fn(text: str, offset: int) -> str:
    names = FN_RE.findall(text[:offset])
    return names[-1] if names else "<module>"


def check(root: Path) -> list[str]:
    src = root / WORKER_SRC
    failures = []
    seen_allowed = set()
    for path in sorted(src.rglob("*.rs")):
        rel = path.relative_to(src).as_posix()
        if rel.endswith(("unit_tests.rs", "/tests.rs")) or "/tests/" in rel:
            continue
        text = path.read_text()
        limit = test_code_start(text)
        for match in STRING_RE.finditer(text, 0, limit):
            sql = match.group(2) if match.group(2) is not None else match.group(3)
            reason = unguarded(sql)
            if reason is None:
                continue
            function = enclosing_fn(text, match.start())
            if (rel, function) in ALLOWED:
                seen_allowed.add((rel, function))
                continue
            line = text.count("\n", 0, match.start()) + 1
            failures.append(
                f"{WORKER_SRC / rel}:{line} in fn {function}: SQL {reason} "
                "without pending_account_approvals. Resolve credentials through "
                "auth::find_credential_account_by_email or the OAuth token lookup in "
                "oauth_store, which return the account's approval."
            )
    for stale in sorted(set(ALLOWED) - seen_allowed):
        failures.append(f"stale ALLOWED entry {stale}: no unguarded SQL there any more")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    failures = check(args.root)
    for failure in failures:
        print(failure, file=sys.stderr)
    if failures:
        return 1
    print("pending account guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
