#!/usr/bin/env python3
"""Guard critical SQLite query plans against full-table scans."""

from __future__ import annotations

import re
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


def assert_remote_home_actor_slices(conn: sqlite3.Connection) -> None:
    """Each followed actor is an ordered index walk, then the slices are merged.

    The merge may sort. The per-actor scan must not: `id` has to be in
    `idx_remote_statuses_actor_published`, or SQLite sorts that tie and the
    `LIMIT` no longer stops on the index.
    """
    index_sql = conn.execute(
        "SELECT sql FROM sqlite_master WHERE name = 'idx_remote_statuses_actor_published'"
    ).fetchone()
    if index_sql is None or "id DESC" not in index_sql[0]:
        raise RuntimeError(
            "idx_remote_statuses_actor_published must be "
            f"(actor_uri, published_at DESC, id DESC), got {index_sql}"
        )

    sql = """
        SELECT 'remote' AS source, rs.id AS status_id, rs.published_at AS timestamp
        FROM follows f
        JOIN remote_statuses rs
          ON rs.id IN (
            SELECT rs_actor.id
            FROM remote_statuses rs_actor
            WHERE rs_actor.actor_uri = f.target_actor_uri
              AND rs_actor.visibility IN ('public', 'unlisted', 'private')
              AND rs_actor.published_at <= ?
              AND (rs_actor.published_at < ? OR rs_actor.id < ?)
            ORDER BY rs_actor.published_at DESC, rs_actor.id DESC
            LIMIT ?
          )
        WHERE f.follower_account_id = ?
          AND f.state = 'accepted'
        ORDER BY rs.published_at DESC, rs.id DESC
        LIMIT ?
    """
    plan = plan_text(
        conn,
        sql,
        (
            "2026-01-15T12:00:00Z",
            "2026-01-15T12:00:00Z",
            "rs-50",
            80,
            "acct-1",
            80,
        ),
    )
    label = "home remote followed actors"
    assert_uses_index(plan, label)
    assert_seeks_alias(plan, label, "f")
    assert_seeks_alias(plan, label, "rs_actor")
    if "idx_remote_statuses_actor_published" not in plan:
        raise RuntimeError(f"{label}: per-actor scan did not use the actor index:\n{plan}")
    if "RIGHT PART OF ORDER BY" in plan:
        raise RuntimeError(f"{label}: per-actor ORDER BY still sorts id ties:\n{plan}")
    if "published_at<?" not in plan and "published_at<" not in plan:
        raise RuntimeError(f"{label}: cursor bound was not an index range:\n{plan}")


def assert_retention_prunes_seek_age(conn: sqlite3.Connection) -> None:
    """The hourly retention prune (`federation_retention.rs`) must seek old
    terminal rows by age; a walk of every delivered row costs D1 reads on
    each cron tick."""
    for table, age_column, where in [
        ("outbox_deliveries", "updated_at", "state IN ('delivered', 'failed', 'expanded') AND updated_at"),
        ("outbound_activities", "updated_at", "state IN ('delivered', 'failed') AND updated_at"),
        ("inbox_activities", "processed_at", "processed_at"),
    ]:
        label = f"{table} retention prune"
        plan = plan_text(
            conn,
            f"SELECT rowid FROM {table} WHERE {where} < datetime(CURRENT_TIMESTAMP, ?) LIMIT ?",
            ("-30 days", 500),
        )
        if f"{age_column}<?" not in plan:
            raise RuntimeError(f"{label}: expected a seek on {age_column}, got:\n{plan}")


def assert_no_null_cursor_disjunction(_plan: str, sql: str, label: str) -> None:
    import re

    if re.search(r"\?\d+\s+IS NULL", sql):
        raise RuntimeError(f"{label}: query still uses nullable cursor disjunctions")


STORE_LOCAL_RS = (
    REPO_ROOT / "crates" / "cfwdon-worker" / "src" / "statuses" / "store_local.rs"
)


def rust_str_const(source: str, name: str) -> str:
    match = re.search(rf'const {name}: &str = "(.*?)";', source, re.DOTALL)
    if match is None:
        raise RuntimeError(f"{name} not found in {STORE_LOCAL_RS}")
    return match.group(1)


def assert_thread_queries(conn: sqlite3.Connection) -> None:
    """Run the thread context SQL as written in `store_local.rs` (not a copy)
    on a mixed local/remote thread, so a broken query fails here, not as a 500."""
    source = STORE_LOCAL_RS.read_text()
    columns = rust_str_const(source, "LOCAL_REPLY_COLUMNS")
    tree_cte = rust_str_const(source, "LOCAL_REPLY_TREE_CTE")
    chain = re.search(r'"(WITH RECURSIVE chain.*?)"\n', source, re.DOTALL)
    if chain is None:
        raise RuntimeError("ancestor chain query not found in store_local.rs")
    chain_sql = chain.group(1).replace("{LOCAL_REPLY_COLUMNS}", columns)

    def local(status_id: str, parent: str | None, ts: str) -> None:
        conn.execute(
            """INSERT INTO statuses (
                id, account_id, in_reply_to_id, content_html, text_content, visibility,
                quote_approval_policy, quote_state, created_at
             ) VALUES (?, 'acct-1', ?, '<p>x</p>', 'x', 'public', 'public', 'accepted', ?)""",
            (status_id, parent, ts),
        )

    def remote(status_id: str, parent_uri: str, ts: str) -> None:
        conn.execute(
            """INSERT INTO remote_statuses (
                id, actor_uri, object_uri, url, in_reply_to_uri, content_html,
                visibility, published_at, raw_object_json
             ) VALUES (?, 'https://remote.example/users/bob', ?, ?, ?, '<p>x</p>',
                       'public', ?, '{}')""",
            (status_id, f"https://remote.example/{status_id}", None, parent_uri, ts),
        )

    local("thread-root", None, "2026-03-01T00:00:00Z")
    local("thread-1", "thread-root", "2026-03-01T00:01:00Z")
    local("thread-2", "thread-1", "2026-03-01T00:02:00Z")
    remote("thread-r1", "https://example.com/users/alice/statuses/thread-1", "2026-03-01T00:03:00Z")
    remote("thread-r2", "https://remote.example/thread-r1", "2026-03-01T00:04:00Z")
    local("thread-other", None, "2026-03-01T00:05:00Z")

    ancestors = [row[0] for row in conn.execute(chain_sql, ("thread-2", 256))]
    if ancestors != ["thread-2", "thread-1", "thread-root"]:
        raise RuntimeError(f"ancestor chain: got {ancestors}")

    tree_params = ("thread-root", "https://example.com/statuses/thread-root",
                   "https://example.com/users/", 256)
    tree = sorted(
        conn.execute(f"{tree_cte} SELECT kind, id FROM reply_tree WHERE depth > 0", tree_params)
    )
    expected = [("local", "thread-1"), ("local", "thread-2"),
                ("remote", "thread-r1"), ("remote", "thread-r2")]
    if tree != expected:
        raise RuntimeError(f"reply tree: got {tree}")
    shallow = sorted(
        conn.execute(
            f"{tree_cte} SELECT id FROM reply_tree WHERE depth > 0", tree_params[:3] + (1,)
        )
    )
    if shallow != [("thread-1",)]:
        raise RuntimeError(f"reply tree depth cap: got {shallow}")
    for label, sql in (
        ("ancestor chain", chain_sql),
        ("reply tree", f"{tree_cte} SELECT id FROM reply_tree"),
    ):
        params = ("thread-2", 256) if label == "ancestor chain" else tree_params
        plan = plan_text(conn, sql, params)
        if "SCAN s " in plan or "SCAN rs " in plan or "SCAN statuses" in plan:
            raise RuntimeError(f"{label}: expected index seeks, got:\n{plan}")


RELAYS_RS = REPO_ROOT / "crates" / "cfwdon-worker" / "src" / "relays.rs"


def assert_remote_purge_slices(conn: sqlite3.Connection) -> None:
    """Run the retention purge SQL from `relays.rs` a slice at a time: each run
    reads a bounded slice, keeps referenced statuses, and the cursor moves on."""
    source = RELAYS_RS.read_text()
    slice_sql = rust_str_const(source, "STALE_PUBLIC_REMOTE_SLICE_SQL")
    purge = re.search(
        r"fn stale_public_remote_status_purge_sql\(\) -> &'static str \{.*?\"(DELETE FROM remote_statuses.*?)\"\n\}",
        source,
        re.DOTALL,
    )
    if purge is None:
        raise RuntimeError("stale_public_remote_status_purge_sql not found in relays.rs")
    purge_sql = purge.group(1)

    conn.execute(
        """INSERT INTO remote_actors (
            actor_uri, username, domain, inbox_uri, public_key_id, public_key_pem
         ) VALUES ('https://stale.example/users/s', 's', 'stale.example',
                   'https://stale.example/inbox', 'https://stale.example/users/s#k', 'k')"""
    )
    for index in range(1, 6):
        conn.execute(
            """INSERT INTO remote_statuses (
                id, actor_uri, object_uri, content_html, visibility, published_at,
                raw_object_json, in_reply_to_uri
             ) VALUES (?, 'https://stale.example/users/s', ?, '<p>x</p>', 'public', ?, '{}', ?)""",
            (
                f"stale-{index}",
                f"https://stale.example/statuses/{index}",
                f"2020-01-0{index}T00:00:00Z",
                # stale-5 replies to stale-2, so stale-2 must stay; nothing
                # refers to stale-5, so it goes.
                "https://stale.example/statuses/2" if index == 5 else None,
            ),
        )
    retention = "-7 days"
    cursor = ("", "")
    examined: list[str] = []
    for _ in range(3):
        rows = conn.execute(slice_sql, (retention, cursor[0], cursor[1], 2)).fetchall()
        stale_rows = [row for row in rows if row[0].startswith("stale-")]
        examined.extend(row[0] for row in stale_rows)
        if not rows:
            break
        last = rows[-1]
        conn.execute(purge_sql, (retention, cursor[0], cursor[1], last[1], last[0]))
        cursor = (last[1], last[0])
    remaining = sorted(
        row[0] for row in conn.execute("SELECT id FROM remote_statuses WHERE id LIKE 'stale-%'")
    )
    if remaining != ["stale-2"]:
        raise RuntimeError(f"remote purge kept {remaining}, expected only stale-2")
    followed = conn.execute(
        "SELECT COUNT(*) FROM remote_statuses WHERE id LIKE 'rs-%'"
    ).fetchone()[0]
    if followed != 120:
        raise RuntimeError(f"remote purge deleted a followed author's statuses: {followed} left")
    if examined[:2] != ["stale-1", "stale-2"]:
        raise RuntimeError(f"remote purge slices out of order: {examined}")
    plan = plan_text(conn, slice_sql, (retention, "", "", 2000))
    assert_uses_index(plan, "remote purge slice")
    if "SCAN rs " in plan and "USING" not in plan.split("SCAN rs ", 1)[1].split(")", 1)[0]:
        raise RuntimeError(f"remote purge slice scans remote_statuses:\n{plan}")


NOTIFICATIONS_DIR = REPO_ROOT / "crates" / "cfwdon-worker" / "src" / "notifications"
STATUS_INSERT_RS = REPO_ROOT / "crates" / "cfwdon-worker" / "src" / "statuses" / "status_insert.rs"
REMOTE_UPSERT_SQL_RS = (
    REPO_ROOT / "crates" / "cfwdon-worker" / "src" / "remote" / "store" / "upsert_sql.rs"
)
NOTIFICATION_COLLECTOR_RE = re.compile(
    r'async fn (list_\w+_notifications_for_account)\((?:(?!async fn).)*?'
    r'\.prepare\(format!\(\s*"(.*?)",\s*bounds\.clause\("([^"]+)",\s*(\d+)\)',
    re.DOTALL,
)


def notification_collector_sql() -> dict[str, tuple[str, str]]:
    """Each collector's SQL as written in `notifications/*_store.rs`, with its
    time-window placeholder filled with and without bounds."""
    queries: dict[str, tuple[str, str]] = {}
    for file in sorted(NOTIFICATIONS_DIR.glob("*_store.rs")):
        for name, sql, column, first in NOTIFICATION_COLLECTOR_RE.findall(file.read_text()):
            first_param = int(first)
            bounded = sql.replace(
                "{}", f" AND {column} >= ?{first_param} AND {column} < ?{first_param + 1}"
            )
            queries[name] = (sql.replace("{}", ""), bounded)
    return queries


def assert_notification_queries(conn: sqlite3.Connection) -> None:
    """Notifications are derived on every read, so each collector must seek the
    viewer's rows by an index on the notified account, never walk statuses or
    interactions. Run the real collector SQL and the real insert SQL that
    records the notified account, and check what each collector returns."""
    queries = notification_collector_sql()
    if len(queries) < 15:
        raise RuntimeError(f"expected every notification collector, found {sorted(queries)}")

    source = STATUS_INSERT_RS.read_text()
    status_insert = re.search(r'"(INSERT INTO statuses \(.*?quoted_account_id.*?)",', source, re.DOTALL)
    poll_insert = re.search(r'"(INSERT INTO status_polls \(.*?)",', source, re.DOTALL)
    remote_upsert = re.search(
        r'fn remote_status_upsert_sql\(\) -> &\'static str \{\s*"(.*?)"\n\}',
        REMOTE_UPSERT_SQL_RS.read_text(),
        re.DOTALL,
    )
    if status_insert is None or poll_insert is None or remote_upsert is None:
        raise RuntimeError("status, poll or remote status insert SQL not found")

    viewer, other = "acct-2", "acct-3"
    ap = "https://example.com/statuses/"

    def local(status_id: str, account: str, ts: str, quote_of: str | None = None) -> None:
        conn.execute(
            status_insert.group(1),
            (status_id, account, ap + status_id, None, None, quote_of, "<p>x</p>", "x", "",
             "public", 0, None, "public", "accepted", None, None, None, ts),
        )

    local("n-own", viewer, "2026-03-01T00:00:00Z")
    local("n-quote", other, "2026-03-01T00:01:00Z", ap + "n-own")
    local("n-self-quote", viewer, "2026-03-01T00:02:00Z", ap + "n-own")
    local("n-quoting-remote", viewer, "2026-03-01T00:03:00Z", "https://remote.example/statuses/q")
    remote_params = [
        "n-rquote", "https://remote.example/users/bob", "https://remote.example/statuses/n-rquote",
        None, None, ap + "n-own", "<p>x</p>", "x", "", "public", 0, None, "accepted",
        "2026-03-01T00:04:00Z", "{}", "2026-03-01T00:04:00Z", None, None, "[]", None,
    ]
    conn.execute(remote_upsert.group(1), remote_params)
    conn.execute(
        "UPDATE remote_statuses SET edited_at = '2026-03-02T00:00:00Z' WHERE id = 'rs-3'"
    )
    conn.execute(
        "UPDATE statuses SET quote_of_uri = 'https://remote.example/statuses/3' WHERE id = 'n-quoting-remote'"
    )
    recorded = dict(conn.execute(
        "SELECT id, quoted_account_id FROM statuses WHERE id LIKE 'n-%' "
        "UNION ALL SELECT id, quoted_account_id FROM remote_statuses WHERE id = 'n-rquote'"
    ))
    if recorded != {"n-own": None, "n-quote": viewer, "n-self-quote": None,
                    "n-quoting-remote": None, "n-rquote": viewer}:
        raise RuntimeError(f"quoted_account_id recorded at insert: got {recorded}")

    conn.execute(
        """INSERT INTO status_mentions (status_id, mention_key, account_id, username, acct, url, created_at)
           VALUES ('n-quote', 'user2', ?1, 'user2', 'user2', 'u', '2026-03-01T00:01:00Z'),
                  ('n-self-quote', 'user2', ?1, 'user2', 'user2', 'u', '2026-03-01T00:02:00Z')""",
        (viewer,),
    )
    for table, actor_column, actor in (
        ("favourites", "account_id", other),
        ("reblogs", "account_id", other),
        ("remote_favourites", "remote_actor_uri", "https://remote.example/users/bob"),
        ("remote_reblogs", "remote_actor_uri", "https://remote.example/users/bob"),
    ):
        conn.execute(
            f"""INSERT INTO {table} ({actor_column}, status_id, target_uri, notified_account_id, created_at)
                VALUES (?, 'n-own', ?, ?, '2026-03-01 00:05:00')""",
            (actor, ap + "n-own", viewer),
        )
    conn.execute(
        """INSERT INTO favourites (account_id, status_id, target_uri, notified_account_id, created_at)
           VALUES (?, 'n-own', 'self', NULL, '2026-03-01 00:06:00')""",
        (viewer,),
    )
    conn.execute(poll_insert.group(1), ("poll-own", "n-own", 0, 0, "2026-03-02T00:00:00Z",
                                        "2026-03-01T00:00:00Z"))
    conn.execute(poll_insert.group(1), ("poll-voted", "status-1", 1, 0, "2026-03-02T00:00:00Z",
                                        "2026-03-01T00:00:00Z"))
    conn.execute(poll_insert.group(1), ("poll-other", "n-quote", 0, 0, "2026-03-02T00:00:00Z",
                                        "2026-03-01T00:00:00Z"))
    for position in (0, 1):
        conn.execute(
            """INSERT INTO status_poll_options (id, poll_id, title, position)
               VALUES (?, 'poll-voted', 'option', ?)""",
            (f"option-{position}", position),
        )
        conn.execute(
            """INSERT INTO status_poll_votes (id, poll_id, account_id, option_position)
               VALUES (?, 'poll-voted', ?, ?)""",
            (f"vote-{position}", viewer, position),
        )
    authors = dict(conn.execute("SELECT id, account_id FROM status_polls"))
    if authors != {"poll-own": viewer, "poll-voted": "acct-1", "poll-other": other}:
        raise RuntimeError(f"status_polls.account_id recorded at insert: got {authors}")

    expected = {
        "list_favourite_notifications_for_account": [(other, "n-own")],
        "list_reblog_notifications_for_account": [(other, "n-own")],
        "list_remote_favourite_notifications_for_account": [
            ("https://remote.example/users/bob", "n-own")
        ],
        "list_remote_reblog_notifications_for_account": [
            ("https://remote.example/users/bob", "n-own")
        ],
        "list_local_mention_notifications_for_account": [("n-quote",)],
        "list_local_quote_notifications_for_account": [("n-quote",)],
        "list_remote_quote_notifications_for_account": [("n-rquote",)],
        "list_quoted_update_notifications_for_account": [("n-quoting-remote",)],
        "list_poll_notifications_for_account": [("poll-own",), ("poll-voted",)],
    }
    # The index each collector must drive from: an index on the notified
    # account, not on the viewer's own statuses (a walk of their history).
    driving_index = {
        "list_favourite_notifications_for_account": "idx_favourites_notified_created",
        "list_reblog_notifications_for_account": "idx_reblogs_notified_created",
        "list_remote_favourite_notifications_for_account": "idx_remote_favourites_notified_created",
        "list_remote_reblog_notifications_for_account": "idx_remote_reblogs_notified_created",
        "list_local_mention_notifications_for_account": "idx_status_mentions_account_created",
        "list_local_quote_notifications_for_account": "idx_statuses_quoted_account_created",
        "list_remote_quote_notifications_for_account": "idx_remote_statuses_quoted_account_published",
        "list_quoted_update_notifications_for_account": "idx_statuses_account_quoting",
        "list_poll_notifications_for_account": "idx_status_polls_account_expires",
        "list_remote_follow_notifications_for_account": "idx_followers_account_created",
    }
    # A month either side: the bound sorts the same against `YYYY-MM-DD HH:MM:SS`
    # and ISO timestamps, so one window serves both stored formats.
    window = ("2026-02-01T00:00:00", "2026-04-01T00:00:00")
    for name, (sql, bounded) in queries.items():
        for variant, params in (("unbounded", (viewer, 20)), ("bounded", (viewer, 20) + window)):
            text = sql if variant == "unbounded" else bounded
            try:
                plan = plan_text(conn, text, params)
            except sqlite3.Error as error:
                raise RuntimeError(f"{name} ({variant}): {error}") from error
            scans = [line for line in plan.splitlines() if "SCAN " in line]
            if scans:
                raise RuntimeError(f"{name} ({variant}): expected index seeks, got:\n{plan}")
            if name in driving_index and driving_index[name] not in plan:
                raise RuntimeError(
                    f"{name} ({variant}): expected a seek on {driving_index[name]}, got:\n{plan}"
                )
            if name in expected:
                rows = [tuple(row[: len(expected[name][0])]) for row in conn.execute(text, params)]
                if name == "list_poll_notifications_for_account":
                    rows = sorted(rows)
                if rows != expected[name]:
                    raise RuntimeError(f"{name} ({variant}): got {rows}, expected {expected[name]}")


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
    for index in range(1, 121):
        conn.execute(
            """INSERT INTO remote_statuses (
                id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri,
                quote_of_uri, content_html, visibility, published_at, raw_object_json
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (
                f"rs-{index}",
                "https://remote.example/users/bob",
                f"https://remote.example/statuses/{index}",
                f"https://remote.example/@bob/{index}",
                None if index == 1 else f"https://remote.example/statuses/{index - 1}",
                None if index % 5 else f"https://remote.example/statuses/{index - 1}",
                None if index % 7 else f"https://remote.example/statuses/{index - 1}",
                "<p>x</p>",
                "public",
                f"2026-01-{(index % 28) + 1:02d}T12:00:00Z",
                "{}",
            ),
        )
    conn.execute(
        """INSERT INTO followers (id, account_id, actor_uri, inbox_uri)
           VALUES (?, ?, ?, ?)""",
        (
            "fl-1",
            "acct-1",
            "https://remote.example/users/bob",
            "https://remote.example/inbox",
        ),
    )
    conn.execute("ANALYZE")


def main() -> int:
    checks: list[tuple[str, str, tuple[object, ...], str | None]] = [
        (
            "media metadata backfill page walks its index",
            """SELECT object_key, attempts FROM media_metadata_backfill
               ORDER BY attempts, object_key LIMIT ?""",
            (200,),
            None,
        ),
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
            "list timeline local members seekable",
            """WITH members AS (
                 SELECT a.id AS account_id
                 FROM account_list_memberships m
                 CROSS JOIN accounts a ON a.id = m.target_account_ref
                 WHERE m.list_id = ?1
                 UNION
                 SELECT a.id AS account_id
                 FROM account_list_memberships m
                 CROSS JOIN accounts a
                   ON a.username = substr(m.target_account_ref, 1, instr(m.target_account_ref, '@') - 1)
                 WHERE m.list_id = ?1
                   AND m.target_account_ref = a.username || '@' || ?2
               )
               SELECT s.id FROM members mb
               CROSS JOIN statuses s ON s.account_id = mb.account_id
               WHERE s.visibility = 'public'
                 AND s.created_at <= ?3 AND (s.created_at < ?3 OR s.id < ?4)
               ORDER BY s.created_at DESC, s.id DESC LIMIT ?5""",
            ("list-1", "example.com", "2026-01-15T12:00:00Z", "status-50", 80),
            "a,s",
        ),
        (
            "list timeline remote members seekable",
            """WITH members AS (
                 SELECT ra.actor_uri AS actor_uri
                 FROM account_list_memberships m
                 CROSS JOIN remote_actors ra ON ra.actor_uri = m.target_account_ref
                 WHERE m.list_id = ?1
                 UNION
                 SELECT actor_uri FROM (
                     SELECT (
                         SELECT ra.actor_uri
                         FROM remote_actors ra
                         WHERE lower(ra.username) = lower(substr(m.target_account_ref, 1, instr(m.target_account_ref, '@') - 1))
                           AND lower(ra.domain) = lower(substr(m.target_account_ref, instr(m.target_account_ref, '@') + 1))
                     ) AS actor_uri
                     FROM account_list_memberships m
                     WHERE m.list_id = ?1
                       AND instr(m.target_account_ref, '@') > 0
                 )
                 WHERE actor_uri IS NOT NULL
               )
               SELECT rs.id FROM members mb
               CROSS JOIN remote_statuses rs ON rs.actor_uri = mb.actor_uri
               WHERE rs.visibility = 'public'
                 AND rs.published_at <= ?2 AND (rs.published_at < ?2 OR rs.id < ?3)
               ORDER BY rs.published_at DESC, rs.id DESC LIMIT ?4""",
            ("list-1", "2026-01-15T12:00:00Z", "status-50", 80),
            "ra,rs",
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
            "remote status counts join seeks pk",
            """SELECT rs.id, COALESCE(rsc.favourites_count, 0)
               FROM remote_statuses rs
               LEFT JOIN remote_status_counts rsc ON rsc.remote_status_id = rs.id
               WHERE rs.id = ?""",
            ("id-1",),
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
        (
            "remote status by object_uri",
            """SELECT id FROM remote_statuses WHERE object_uri = ? LIMIT 1""",
            ("https://remote.example/statuses/1",),
            None,
        ),
        (
            "remote status by url",
            """SELECT id FROM remote_statuses WHERE url = ? LIMIT 1""",
            ("https://remote.example/@bob/1",),
            None,
        ),
        (
            "remote status url or object_uri union",
            """SELECT id FROM remote_statuses
               WHERE object_uri IN (SELECT value FROM json_each(?))
               UNION
               SELECT id FROM remote_statuses
               WHERE url IN (SELECT value FROM json_each(?))""",
            (
                '["https://remote.example/statuses/1"]',
                '["https://remote.example/statuses/1"]',
            ),
            None,
        ),
        (
            "remote status boost_of_uri lookup",
            """SELECT id FROM remote_statuses WHERE boost_of_uri = ? LIMIT 1""",
            ("https://remote.example/statuses/1",),
            None,
        ),
        (
            "remote status in_reply_to_uri lookup",
            """SELECT id FROM remote_statuses WHERE in_reply_to_uri = ? LIMIT 1""",
            ("https://remote.example/statuses/1",),
            None,
        ),
        (
            "inbound Flag report status links seek the report",
            """SELECT r.id, j.value
               FROM reports r, json_each(?4) j
               WHERE r.id = ?1 OR (r.uri = ?2 AND r.target_account_id = ?3)""",
            ("report-1", "https://remote.example/flags/1", "acct-1", '["status-1"]'),
            "r",
        ),
        (
            "followers by actor_uri",
            """SELECT 1 FROM followers WHERE actor_uri = ? LIMIT 1""",
            ("https://remote.example/users/bob",),
            None,
        ),
        (
            "oauth token decode seeks the pending approval",
            """SELECT t.oauth_app_id, a.id, p.account_id AS pending_account_id
               FROM oauth_access_tokens t
               LEFT JOIN accounts a ON a.id = t.account_id
               LEFT JOIN pending_account_approvals p ON p.account_id = a.id
               WHERE t.access_token_hash = ?1
                 AND (t.expires_at IS NULL OR t.expires_at > ?2)
               LIMIT 1""",
            ("sha256:abc", 0),
            "t,a,p",
        ),
        (
            "auth0 sign-in seeks the pending approval",
            """SELECT a.id, p.account_id AS pending_account_id
               FROM accounts a
               LEFT JOIN pending_account_approvals p ON p.account_id = a.id
               WHERE a.access_email = ?1
               LIMIT 1""",
            ("alice@example.com",),
            "a,p",
        ),
        (
            "public account lookup by id skips pending accounts by key",
            """SELECT id FROM accounts
               WHERE id = ?1 AND NOT EXISTS (
                   SELECT 1 FROM pending_account_approvals p WHERE p.account_id = accounts.id
               )
               LIMIT 1""",
            ("acct-1",),
            "accounts,p",
        ),
        (
            "public account lookup by username skips pending accounts by key",
            """SELECT id FROM accounts
               WHERE username = ?1 AND NOT EXISTS (
                   SELECT 1 FROM pending_account_approvals p WHERE p.account_id = accounts.id
               )
               LIMIT 1""",
            ("alice",),
            "accounts,p",
        ),
        (
            "silenced authors the viewer follows",
            """SELECT target_actor_uri
               FROM follows
               WHERE follower_account_id = ?1
                 AND state = 'accepted'
                 AND target_actor_uri IN (SELECT value FROM json_each(?2))""",
            ("acct-1", '["https://remote.example/users/bob"]'),
            None,
        ),
        (
            "directory remote actor last status",
            """SELECT MAX(published_at) FROM remote_statuses WHERE actor_uri = ?""",
            ("https://remote.example/users/bob",),
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
                    for alias in seek_alias.split(","):
                        assert_seeks_alias(plan, label, alias)
                assert_no_null_cursor_disjunction(plan, sql, label)
            assert_remote_home_actor_slices(conn)
            assert_retention_prunes_seek_age(conn)
            assert_thread_queries(conn)
            assert_remote_purge_slices(conn)
            assert_notification_queries(conn)
    except RuntimeError as error:
        print(f"query plan check failed: {error}", file=sys.stderr)
        return 1

    print(f"query plan check ok: verified {len(checks)} critical queries")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
