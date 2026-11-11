#!/usr/bin/env python3
"""Fail on worker code that can resolve a credential to a pending account.

An account waiting for approval (a `pending_account_approvals` row) must not
act through its OAuth token or its e-mail, and public surfaces must not show
it. The worker crate keeps that true in two places, and this check covers
both outside test code:

SQL. Each SQL text (a string literal, a `concat!` of literals, with `{CONST}`
placeholders replaced by the crate's `&str` constants and SQL comments
removed) fails when it

- names both `oauth_access_tokens` and `accounts`, in any order or nesting, or
- compares `access_email` in a predicate (`=`, `IN`, `LIKE`, `lower(...)`,
  any form after `WHERE`, `ON`, `AND` or `OR`), or
- reads `accounts` with a runtime column (`WHERE {column} = ...`),

unless it also carries a real pending filter: a join to, or an `EXISTS` or
`IN` subquery over, `pending_account_approvals`.

Lookups. `AccountLookup::IncludingPending` returns pending accounts, so each
use must be listed in ALLOWED_INCLUDING_PENDING. `find_account_where` is
private to `auth/account_store.rs` and keys on the `AccountKey` enum, which
has no e-mail variant; a call anywhere else fails.

A lookup that must see every account regardless of approval goes in an
ALLOWED table with the reason.

Usage: check_pending_account_guard.py [--root REPO_ROOT] [--self-test]

--self-test runs the check on BYPASSES, each of which must fail, and on
GUARDED, none of which may.
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from pathlib import Path

WORKER_SRC = Path("crates/cfwdon-worker/src")

# (path relative to WORKER_SRC, enclosing fn) -> why its SQL may skip the approval.
ALLOWED_SQL = {
    ("auth/account_store.rs", "find_account_id_holding_email"): (
        "uniqueness and sign-in existence checks must see pending accounts; "
        "it returns only an id, never an account to act as"
    ),
    ("auth/account_store.rs", "find_account_where"): (
        "keys on the AccountKey enum (id or username) and adds APPROVED_ONLY "
        "unless the caller passed AccountLookup::IncludingPending, which this "
        "check allowlists separately"
    ),
}

# (path relative to WORKER_SRC, enclosing fn) -> why it may see pending accounts.
ALLOWED_INCLUDING_PENDING = {
    ("auth/account_store.rs", "find_account_where"): "applies the lookup",
    ("auth/account_store.rs", "ensure_account_keys"): (
        "reloads the account the caller already holds after writing its keys"
    ),
    ("meta_placeholder_routes/account_registration.rs", "create_account_placeholder_response"): (
        "sign-up uniqueness: a pending account keeps its username"
    ),
    ("oauth_apps/authorize_route.rs", "authorize_account_by_password"): (
        "Mastodon signs a pending account in with its password and issues its "
        "code; the token lookup refuses the token afterwards"
    ),
    ("oauth_apps/token_routes.rs", "oauth_authorization_code_token_response"): (
        "Mastodon issues a pending account its token; the token lookup "
        "refuses it afterwards"
    ),
    ("admin_api/accounts.rs", "review_account_response"): "the admin review of a pending account",
}

STRING_RE = re.compile(r'r(#*)"(.*?)"\1|"((?:\\.|[^"\\])*)"', re.S)
CONCAT_RE = re.compile(r"\bconcat!\s*\(")
CONST_RE = re.compile(
    r'\bconst\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:\'static\s+)?str\s*=\s*'
    r'(?:r(#*)"(.*?)"\2|"((?:\\.|[^"\\])*)")\s*;',
    re.S,
)
FN_RE = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)")
PLACEHOLDER_RE = re.compile(r"\{([A-Za-z_][A-Za-z0-9_]*)\}")
BLOCK_COMMENT_RE = re.compile(r"/\*.*?\*/", re.S)
LINE_COMMENT_RE = re.compile(r"--[^\n]*")

TOKENS_RE = re.compile(r"\boauth_access_tokens\b", re.I)
ACCOUNTS_RE = re.compile(r"\baccounts\b", re.I)
ACCOUNTS_TABLE_RE = re.compile(r"\b(?:FROM|JOIN|UPDATE|INTO)\s+accounts\b", re.I)
EMAIL_PREDICATE_RE = re.compile(r"\b(?:WHERE|ON|AND|OR)\b[^;]*?\baccess_email\b", re.I | re.S)
RUNTIME_COLUMN_RE = re.compile(r"\bWHERE\b[^;]*?\{[A-Za-z_][A-Za-z0-9_]*\}\s*(?:=|\bIN\b|\bLIKE\b)", re.I | re.S)
PENDING_FILTER_RE = re.compile(
    r"\bJOIN\s+pending_account_approvals\b"
    r"|\b(?:EXISTS|IN)\s*\(\s*SELECT\b[^()]*?\bFROM\s+pending_account_approvals\b",
    re.I | re.S,
)
INCLUDING_PENDING_RE = re.compile(r"\bAccountLookup::IncludingPending\b")
FIND_WHERE_CALL_RE = re.compile(r"\bfind_account_where\s*\(")
FIND_WHERE_HOME = "auth/account_store.rs"


def test_code_start(text: str) -> int:
    """Offset of the first `#[cfg(test)]` module; test fixtures follow it."""
    match = re.search(r"(?m)^\s*#\[cfg\(test\)\]\s*\n\s*mod\s", text)
    return match.start() if match else len(text)


def literal_text(match: re.Match[str]) -> str:
    return match.group(2) if match.group(2) is not None else match.group(3)


def strip_line_comments(text: str) -> str:
    """Drop `//` comments outside string literals, so prose is not SQL."""
    out, pos = [], 0
    for match in re.finditer(r'r(#*)"(.*?)"\1|"(?:\\.|[^"\\])*"|//[^\n]*', text, re.S):
        out.append(text[pos : match.start()])
        token = match.group(0)
        out.append(" " * len(token) if token.startswith("//") else token)
        pos = match.end()
    out.append(text[pos:])
    return "".join(out)


def sql_units(text: str, limit: int) -> list[tuple[int, str]]:
    """(offset, SQL text) per literal, with each `concat!` joined into one."""
    units, covered = [], []
    for match in CONCAT_RE.finditer(text, 0, limit):
        depth, pos = 1, match.end()
        while pos < len(text) and depth:
            depth += {"(": 1, ")": -1}.get(text[pos], 0)
            pos += 1
        parts = [literal_text(m) for m in STRING_RE.finditer(text, match.end(), pos)]
        units.append((match.start(), "".join(parts)))
        covered.append((match.start(), pos))
    for match in STRING_RE.finditer(text, 0, limit):
        if not any(start <= match.start() < end for start, end in covered):
            units.append((match.start(), literal_text(match)))
    return units


def expand(sql: str, constants: dict[str, str], depth: int = 0) -> str:
    if depth > 4:
        return sql
    return PLACEHOLDER_RE.sub(
        lambda m: expand(constants[m.group(1)], constants, depth + 1)
        if m.group(1) in constants
        else m.group(0),
        sql,
    )


def unguarded(sql: str) -> str | None:
    sql = LINE_COMMENT_RE.sub(" ", BLOCK_COMMENT_RE.sub(" ", sql))
    if PENDING_FILTER_RE.search(sql):
        return None
    if TOKENS_RE.search(sql) and ACCOUNTS_RE.search(sql):
        return "names oauth_access_tokens and accounts"
    if EMAIL_PREDICATE_RE.search(sql):
        return "compares access_email"
    if ACCOUNTS_TABLE_RE.search(sql) and RUNTIME_COLUMN_RE.search(sql):
        return "reads accounts by a runtime column"
    return None


def enclosing_fn(text: str, offset: int) -> str:
    names = FN_RE.findall(text[:offset])
    return names[-1] if names else "<module>"


def check(root: Path) -> list[str]:
    src = root / WORKER_SRC
    files = []
    for path in sorted(src.rglob("*.rs")):
        rel = path.relative_to(src).as_posix()
        if rel.endswith(("unit_tests.rs", "/tests.rs")) or "/tests/" in rel:
            continue
        text = strip_line_comments(path.read_text())
        files.append((rel, text, test_code_start(text)))

    constants = {}
    for _, text, limit in files:
        for match in CONST_RE.finditer(text, 0, limit):
            value = match.group(3) if match.group(3) is not None else match.group(4)
            constants.setdefault(match.group(1), value)

    failures, seen_sql, seen_pending = [], set(), set()

    def fail(rel: str, text: str, offset: int, message: str) -> None:
        line = text.count("\n", 0, offset) + 1
        failures.append(f"{WORKER_SRC / rel}:{line} in fn {enclosing_fn(text, offset)}: {message}")

    for rel, text, limit in files:
        for offset, sql in sql_units(text, limit):
            reason = unguarded(expand(sql, constants))
            if reason is None:
                continue
            key = (rel, enclosing_fn(text, offset))
            if key in ALLOWED_SQL:
                seen_sql.add(key)
                continue
            fail(
                rel,
                text,
                offset,
                f"SQL {reason} without a pending_account_approvals filter. Resolve "
                "credentials through auth::find_credential_account_by_email or the "
                "OAuth token lookup in oauth_store, which return the account's approval.",
            )
        for match in INCLUDING_PENDING_RE.finditer(text, 0, limit):
            key = (rel, enclosing_fn(text, match.start()))
            if key in ALLOWED_INCLUDING_PENDING:
                seen_pending.add(key)
                continue
            fail(
                rel,
                text,
                match.start(),
                "AccountLookup::IncludingPending returns pending accounts. Use "
                "AccountLookup::Approved, or add this fn to ALLOWED_INCLUDING_PENDING "
                "in scripts/check_pending_account_guard.py with the reason.",
            )
        if rel != FIND_WHERE_HOME:
            for match in FIND_WHERE_CALL_RE.finditer(text, 0, limit):
                fail(
                    rel,
                    text,
                    match.start(),
                    "find_account_where is private to auth/account_store.rs. Call "
                    "find_account_by_id or find_account_by_username.",
                )

    for stale in sorted(set(ALLOWED_SQL) - seen_sql):
        failures.append(f"stale ALLOWED_SQL entry {stale}: no unguarded SQL there any more")
    for stale in sorted(set(ALLOWED_INCLUDING_PENDING) - seen_pending):
        failures.append(
            f"stale ALLOWED_INCLUDING_PENDING entry {stale}: no IncludingPending there any more"
        )
    return failures


# One fn per way past an earlier version of this check.
BYPASSES = r'''
fn reversed_join() { db.prepare("SELECT a.* FROM accounts a JOIN oauth_access_tokens t ON t.account_id = a.id WHERE t.access_token_hash = ?1"); }
fn subquery() { db.prepare("SELECT * FROM accounts WHERE id = (SELECT account_id FROM oauth_access_tokens WHERE access_token_hash = ?1)"); }
fn formatted_col() { let c = "access_email"; db.prepare(format!("SELECT * FROM accounts WHERE {c} = ?1")); }
fn lower_email() { db.prepare("SELECT * FROM accounts WHERE lower(access_email) = lower(?1)"); }
fn in_email() { db.prepare("SELECT * FROM accounts WHERE access_email IN (?1)"); }
fn helper_including_pending() { find_account_where(db, "access_email", email, AccountLookup::IncludingPending); }
fn mention_only() { db.prepare("SELECT a.* FROM oauth_access_tokens t JOIN accounts a ON a.id=t.account_id /* pending_account_approvals */ WHERE t.access_token_hash = ?1"); }
fn split() { db.prepare(concat!("SELECT a.* FROM oauth_access_tokens t ", "JOIN accounts a ON a.id = t.account_id WHERE t.access_token_hash = ?1")); }
fn control() { db.prepare("SELECT * FROM accounts WHERE access_email = ?1"); }
fn const_split() { db.prepare(format!("{TOKEN_FROM} JOIN accounts a ON a.id = t.account_id")); }
const TOKEN_FROM: &str = "SELECT a.* FROM oauth_access_tokens t";
'''

GUARDED = r'''
fn token_with_approval() { db.prepare("SELECT a.*, p.account_id FROM oauth_access_tokens t JOIN accounts a ON a.id = t.account_id LEFT JOIN pending_account_approvals p ON p.account_id = a.id"); }
fn email_with_approval() { db.prepare(format!("{ACCOUNT_SELECT} WHERE access_email = ?1 AND {APPROVED_ONLY}")); }
fn by_id() { db.prepare("SELECT access_email FROM accounts WHERE id = ?1"); }
fn update_email() { db.prepare("UPDATE accounts SET access_email = ?1 WHERE id = ?2"); }
const ACCOUNT_SELECT: &str = "SELECT id FROM accounts";
const APPROVED_ONLY: &str = "NOT EXISTS (SELECT 1 FROM pending_account_approvals p WHERE p.account_id = accounts.id)";
'''


def flagged_fns(corpus: str) -> set[str]:
    with tempfile.TemporaryDirectory() as root:
        src = Path(root) / WORKER_SRC
        src.mkdir(parents=True)
        (src / "corpus.rs").write_text(corpus)
        return {
            match.group(1)
            for failure in check(Path(root))
            if (match := re.search(r" in fn (\w+):", failure))
        }


def self_test() -> list[str]:
    expected = set(FN_RE.findall(BYPASSES))
    failures = [f"bypass not caught: fn {name}" for name in sorted(expected - flagged_fns(BYPASSES))]
    failures += [f"guarded SQL flagged: fn {name}" for name in sorted(flagged_fns(GUARDED))]
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    failures = self_test() if args.self_test else check(args.root)
    for failure in failures:
        print(failure, file=sys.stderr)
    if failures:
        return 1
    print("pending account guard self-test ok" if args.self_test else "pending account guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
