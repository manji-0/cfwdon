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


def assert_seeks_alias(plan: str, label: str, alias: str) -> None:
    """A full index SCAN still reads every row. Queries that run per status
    (stream fan-out) must seek their driving table, not walk it."""
    if f"SCAN {alias} " in plan:
        raise RuntimeError(f"{label}: expected a seek on `{alias}`, got:\n{plan}")
    if f"SEARCH {alias} " not in plan:
        raise RuntimeError(f"{label}: expected a seek on `{alias}`, got:\n{plan}")


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
    for index in range(2, 102):
        conn.execute(
            "INSERT INTO accounts (id, username, access_email, display_name) VALUES (?, ?, ?, ?)",
            (f"acct-{index}", f"user{index}", f"user{index}@example.com", f"User {index}"),
        )
        conn.execute(
            """INSERT INTO follows (
                id, follower_account_id, target_account_id, target_actor_uri, state
             ) VALUES (?, ?, ?, ?, ?)""",
            (
                f"follow-{index}",
                f"acct-{index}",
                "acct-1",
                "https://remote.example/users/bob",
                "accepted",
            ),
        )
        conn.execute(
            """INSERT INTO scheduled_statuses (
                id, account_id, text_content, visibility, scheduled_at
             ) VALUES (?, ?, ?, ?, ?)""",
            (
                f"sched-{index}",
                "acct-1",
                "later",
                "public",
                f"2026-02-{(index % 28) + 1:02d}T12:00:00Z",
            ),
        )
    conn.execute("ANALYZE")


def main() -> int:
    checks: list[tuple[str, str, tuple[object, ...], str | None]] = [
        (
            "public timeline seekable",
            """SELECT id FROM statuses
               WHERE visibility = 'public'
                 AND created_at <= ?
                 AND (created_at < ? OR id < ?)
               ORDER BY created_at DESC, id DESC
               LIMIT ?""",
            ("2026-01-15T12:00:00Z", "2026-01-15T12:00:00Z", "status-50", 20),
            None,
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
            None,
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
            None,
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
            None,
        ),
        (
            "stream fan-out followers by account",
            """SELECT f.follower_account_id
               FROM follows f
               WHERE f.target_account_id = ?
                 AND f.state = 'accepted'
                 AND NOT EXISTS (
                     SELECT 1 FROM blocks b
                     WHERE b.blocker_account_id = f.follower_account_id
                       AND b.target_actor_uri = ?
                 )
                 AND NOT EXISTS (
                     SELECT 1 FROM mutes m
                     WHERE m.account_id = f.follower_account_id
                       AND m.target_actor_uri = ?
                       AND (m.expires_at IS NULL OR m.expires_at > CURRENT_TIMESTAMP)
                 )
               ORDER BY f.created_at DESC, f.rowid DESC
               LIMIT ?""",
            ("acct-1", "https://example.com/users/alice", "https://example.com/users/alice", 201),
            "f",
        ),
        (
            "stream fan-out followers by actor uri",
            """SELECT f.follower_account_id
               FROM follows f
               WHERE f.target_actor_uri = ?
                 AND f.state = 'accepted'
                 AND NOT EXISTS (
                     SELECT 1 FROM blocks b
                     WHERE b.blocker_account_id = f.follower_account_id
                       AND b.target_actor_uri = ?
                 )
                 AND NOT EXISTS (
                     SELECT 1 FROM mutes m
                     WHERE m.account_id = f.follower_account_id
                       AND m.target_actor_uri = ?
                       AND (m.expires_at IS NULL OR m.expires_at > CURRENT_TIMESTAMP)
                 )
               ORDER BY f.created_at DESC, f.rowid DESC
               LIMIT ?""",
            (
                "https://remote.example/users/bob",
                "https://remote.example/users/bob",
                "https://remote.example/users/bob",
                201,
            ),
            "f",
        ),
        (
            "scheduled statuses due sweep",
            """SELECT id FROM scheduled_statuses
               WHERE scheduled_at <= ?
                 AND (claimed_at IS NULL OR claimed_at <= ?)
               ORDER BY scheduled_at ASC, id ASC
               LIMIT ?""",
            ("2026-02-15T12:00:00Z", "2026-02-15T11:55:00Z", 32),
            None,
        ),
        (
            "outbox generic claim",
            """SELECT id FROM outbox_deliveries
               WHERE state = 'queued' AND target_inbox IS NULL
                 AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
               ORDER BY created_at ASC LIMIT ?""",
            (16,),
            None,
        ),
    ]

    try:
        with sqlite3.connect(":memory:") as conn:
            apply_migrations(conn)
            seed(conn)
            for label, sql, params, seek_alias in checks:
                plan = plan_text(conn, sql, params)
                assert_uses_index(plan, label)
                if seek_alias is not None:
                    assert_seeks_alias(plan, label, seek_alias)
                assert_no_null_cursor_disjunction(plan, sql, label)
    except RuntimeError as error:
        print(f"query plan check failed: {error}", file=sys.stderr)
        return 1

    print(f"query plan check ok: verified {len(checks)} critical queries")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
