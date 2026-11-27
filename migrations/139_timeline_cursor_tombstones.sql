-- Timeline paging cursors are status ids, and a status id carries no time: the
-- server looks the status up to find where the page starts. A client that pages
-- from a status deleted after it rendered the page (a deleted post, an unboost,
-- a remote Delete or Undo Announce) used to get an empty page, so it stopped
-- paging. Each delete now keeps the id and its timeline timestamp here, and the
-- cursor lookup falls back to this table. Rows expire after a few days; see
-- `purge_expired_timeline_cursor_tombstones`.
CREATE TABLE IF NOT EXISTS timeline_cursor_tombstones (
    id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL,
    deleted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_timeline_cursor_tombstones_deleted_at
    ON timeline_cursor_tombstones (deleted_at);
