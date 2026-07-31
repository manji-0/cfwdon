#!/usr/bin/env python3
"""Guard critical SQLite query plans against full-table scans."""

from __future__ import annotations

import sqlite3
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
MIGRATIONS_DIR = REPO_ROOT / "migrations"


def apply_migrations(conn: sqlite3.Connection) -> None:
    conn.execute("PRAGMA foreign_keys = ON")
    for file in sorted(MIGRATIONS_DIR.glob("*.sql")):
        conn.executescript(file.read_text())


def plan_text(conn: sqlite3.Connection, sql: str, params: tuple[object, ...] = ()) -> str:
    rows = conn.execute(f"EXPLAIN QUERY PLAN {sql}", params).fetchall()
    return "\n".join(str(row) for row in rows)


def assert_uses_index(plan: str, label: str) -> None:
    if "USING INDEX" not in plan and "USING COVERING INDEX" not in plan:
        raise RuntimeError(f"{label}: expected an index scan, got:\n{plan}")


def assert_no_null_cursor_disjunction(_plan: str, sql: str, label: str) -> None:
    import re

    if re.search(r"\?\d+\s+IS NULL", sql):
        raise RuntimeError(f"{label}: query still uses nullable cursor disjunctions")


def seed(conn: sqlite3.Connection) -> None:
    conn.execute(
        "INSERT INTO accounts (id, username, access_email, display_name) VALUES (?, ?, ?, ?)",
        ("acct-1", "alice", "alice@example.com", "Alice"),
    )
    conn.execute(
        """INSERT INTO remote_actors (
            actor_uri, username, domain, inbox_uri, public_key_id, public_key_pem
         ) VALUES (?, ?, ?, ?, ?, ?)""",
        (
            "https://remote.example/users/bob",
            "bob",
            "remote.example",
            "https://remote.example/inbox",
            "https://remote.example/users/bob#main-key",
            "public-key",
        ),
    )
    for index in range(1, 101):
        ts = f"2026-01-{(index % 28) + 1:02d}T12:00:00Z"
        conn.execute(
            """INSERT INTO statuses (
                id, account_id, ap_id, content_html, text_content, visibility,
                quote_approval_policy, quote_state, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                f"status-{index}",
                "acct-1",
                f"https://example.com/statuses/{index}",
                "<p>x</p>",
                "x",
                "public",
                "public",
                "accepted",
                ts,
            ),
        )
    conn.execute(
        """INSERT INTO follows (
            id, follower_account_id, target_account_id, target_actor_uri, state
         ) VALUES (?, ?, ?, ?, ?)""",
        ("follow-1", "acct-1", "acct-1", "https://remote.example/users/bob", "accepted"),
    )
    conn.execute("ANALYZE")


def main() -> int:
    checks: list[tuple[str, str, tuple[object, ...]]] = [
        (
            "public timeline seekable",
            """SELECT id FROM statuses
               WHERE visibility = 'public'
                 AND created_at <= ?
                 AND (created_at < ? OR id < ?)
               ORDER BY created_at DESC, id DESC
               LIMIT ?""",
            ("2026-01-15T12:00:00Z", "2026-01-15T12:00:00Z", "status-50", 20),
        ),
        (
            "home followed seekable",
            """SELECT s.id FROM statuses s
               JOIN follows f ON s.account_id = f.target_account_id
               WHERE f.follower_account_id = ? AND f.state = 'accepted'
                 AND s.visibility IN ('public', 'unlisted', 'private')
                 AND s.created_at <= ? AND (s.created_at < ? OR s.id < ?)
               ORDER BY s.created_at DESC, s.id DESC LIMIT ?""",
            ("acct-1", "2026-01-15T12:00:00Z", "2026-01-15T12:00:00Z", "status-50", 20),
        ),
        (
            "status quotes seekable",
            """SELECT id FROM statuses
               WHERE quote_of_uri = ?
                 AND quote_state = 'accepted'
                 AND created_at <= ?
                 AND (created_at < ? OR id < ?)
               ORDER BY created_at DESC, id DESC
               LIMIT ?""",
            ("https://example.com/statuses/1", "2026-01-15T12:00:00Z", "2026-01-15T12:00:00Z", "status-50", 20),
        ),
        (
            "status search seekable",
            """SELECT id FROM statuses
               WHERE lower(text_content) LIKE ?
                 AND created_at <= ?
                 AND (created_at < ? OR id < ?)
               ORDER BY created_at DESC, id DESC
               LIMIT ?""",
            ("%rust%", "2026-01-15T12:00:00Z", "2026-01-15T12:00:00Z", "status-50", 20),
        ),
        (
            "outbox generic claim",
            """SELECT id FROM outbox_deliveries
               WHERE state = 'queued' AND target_inbox IS NULL
                 AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
               ORDER BY created_at ASC LIMIT ?""",
            (16,),
        ),
    ]

    try:
        with sqlite3.connect(":memory:") as conn:
            apply_migrations(conn)
            seed(conn)
            for label, sql, params in checks:
                plan = plan_text(conn, sql, params)
                assert_uses_index(plan, label)
                assert_no_null_cursor_disjunction(plan, sql, label)
    except RuntimeError as error:
        print(f"query plan check failed: {error}", file=sys.stderr)
        return 1

    print(f"query plan check ok: verified {len(checks)} critical queries")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
