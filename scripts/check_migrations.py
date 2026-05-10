#!/usr/bin/env python3
"""Validate D1 migrations against SQLite before deployment."""

from __future__ import annotations

import re
import sqlite3
import sys
from collections import defaultdict
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
MIGRATIONS_DIR = REPO_ROOT / "migrations"
MIGRATION_PREFIX_RE = re.compile(r"^(\d{3})_")

# These duplicate prefixes already exist in the checked-in migration history.
# Renaming them would make Wrangler see different migration files on deployed
# databases, so the guard only rejects newly introduced duplicate prefixes.
LEGACY_DUPLICATE_PREFIXES = {"062", "063", "073"}


def migration_files() -> list[Path]:
    files = sorted(MIGRATIONS_DIR.glob("*.sql"))
    if not files:
        raise RuntimeError(f"no migrations found in {MIGRATIONS_DIR}")
    return files


def check_duplicate_prefixes(files: list[Path]) -> None:
    by_prefix: dict[str, list[str]] = defaultdict(list)
    for file in files:
        match = MIGRATION_PREFIX_RE.match(file.name)
        if not match:
            raise RuntimeError(f"migration file must start with a 3-digit prefix: {file.name}")
        by_prefix[match.group(1)].append(file.name)

    unexpected = {
        prefix: names
        for prefix, names in by_prefix.items()
        if len(names) > 1 and prefix not in LEGACY_DUPLICATE_PREFIXES
    }
    if unexpected:
        details = ", ".join(
            f"{prefix}: {', '.join(names)}" for prefix, names in sorted(unexpected.items())
        )
        raise RuntimeError(f"duplicate migration prefixes are not allowed: {details}")


def apply_migrations(conn: sqlite3.Connection, files: list[Path]) -> None:
    conn.execute("PRAGMA foreign_keys = ON")
    for file in files:
        try:
            conn.executescript(file.read_text())
        except sqlite3.Error as error:
            raise RuntimeError(f"{file.relative_to(REPO_ROOT)} failed: {error}") from error


def check_database(conn: sqlite3.Connection) -> None:
    integrity = conn.execute("PRAGMA integrity_check").fetchone()
    if integrity is None or integrity[0] != "ok":
        raise RuntimeError(f"integrity_check failed: {integrity[0] if integrity else 'no result'}")

    foreign_key_errors = conn.execute("PRAGMA foreign_key_check").fetchall()
    if foreign_key_errors:
        formatted = "; ".join(str(row) for row in foreign_key_errors[:10])
        raise RuntimeError(f"foreign_key_check failed: {formatted}")


def expect_integrity_error(conn: sqlite3.Connection, sql: str, params: tuple[str, ...]) -> None:
    try:
        conn.execute(sql, params)
    except sqlite3.IntegrityError:
        return
    raise RuntimeError(f"expected integrity error for SQL: {sql}")


def check_integrity_guards(conn: sqlite3.Connection) -> None:
    conn.execute(
        "INSERT INTO accounts (id, username, access_email, display_name) VALUES (?, ?, ?, ?)",
        ("acct-1", "alice", "alice@example.com", "Alice"),
    )
    conn.execute(
        """INSERT INTO remote_actors (
            actor_uri,
            username,
            domain,
            inbox_uri,
            public_key_id,
            public_key_pem
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
    conn.execute(
        """INSERT INTO statuses (
            id,
            account_id,
            ap_id,
            content_html,
            text_content,
            visibility,
            quote_approval_policy,
            quote_state
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
        (
            "status-1",
            "acct-1",
            "https://example.com/users/alice/statuses/status-1",
            "<p>Hello #sqlite</p>",
            "Hello #sqlite",
            "public",
            "public",
            "accepted",
        ),
    )
    conn.execute(
        """INSERT INTO remote_statuses (
            id,
            actor_uri,
            object_uri,
            content_html,
            visibility,
            published_at,
            raw_object_json,
            quote_state
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
        (
            "remote-status-1",
            "https://remote.example/users/bob",
            "https://remote.example/users/bob/statuses/1",
            "<p>Remote #sqlite</p>",
            "public",
            "2026-05-10T00:00:00Z",
            "{}",
            "accepted",
        ),
    )

    conn.execute(
        "INSERT INTO favourites (account_id, status_id, target_uri) VALUES (?, ?, ?)",
        ("acct-1", "status-1", "https://example.com/users/alice/statuses/status-1"),
    )
    conn.execute(
        """INSERT INTO media_attachments (
            id,
            account_id,
            object_key,
            content_type,
            focus_x,
            focus_y
         ) VALUES (?, ?, ?, ?, ?, ?)""",
        ("media-1", "acct-1", "media/1", "image/png", 0.0, 0.5),
    )
    conn.execute(
        """INSERT INTO status_hashtags (status_id, tag, account_id, created_at)
         VALUES (?, ?, ?, ?)""",
        ("status-1", "sqlite", "acct-1", "2026-05-10T00:00:00Z"),
    )
    conn.execute(
        """INSERT INTO remote_status_hashtags (status_id, tag, actor_uri, published_at)
         VALUES (?, ?, ?, ?)""",
        (
            "remote-status-1",
            "sqlite",
            "https://remote.example/users/bob",
            "2026-05-10T00:00:00Z",
        ),
    )

    expect_integrity_error(
        conn,
        "INSERT INTO favourites (account_id, target_uri) VALUES (?, ?)",
        ("acct-1", "https://example.com/missing-target"),
    )
    expect_integrity_error(
        conn,
        """INSERT INTO statuses (
            id,
            account_id,
            content_html,
            text_content,
            visibility,
            quote_approval_policy,
            quote_state
         ) VALUES (?, ?, ?, ?, ?, ?, ?)""",
        ("status-bad-visibility", "acct-1", "", "", "friends", "public", "accepted"),
    )
    expect_integrity_error(
        conn,
        """INSERT INTO media_attachments (id, account_id, object_key, content_type, focus_x)
         VALUES (?, ?, ?, ?, ?)""",
        ("media-bad-focus", "acct-1", "media/bad", "image/png", "1.5"),
    )
    expect_integrity_error(
        conn,
        """INSERT INTO remote_status_hashtags (status_id, tag, actor_uri, published_at)
         VALUES (?, ?, ?, ?)""",
        (
            "missing-remote-status",
            "sqlite",
            "https://remote.example/users/bob",
            "2026-05-10T00:00:00Z",
        ),
    )


def main() -> int:
    files = migration_files()
    try:
        check_duplicate_prefixes(files)
        with sqlite3.connect(":memory:") as conn:
            apply_migrations(conn, files)
            check_database(conn)
            check_integrity_guards(conn)
    except RuntimeError as error:
        print(f"migration check failed: {error}", file=sys.stderr)
        return 1

    print(f"migration check ok: applied {len(files)} migrations")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
