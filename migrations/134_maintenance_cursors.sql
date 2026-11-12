-- Keyset cursors for maintenance jobs that walk a large table a slice per
-- run instead of re-reading all of it every time (D1 bills every row read).
-- `published_at` / `id` name the last row a job examined; empty strings sort
-- before every row, so a missing or reset cursor starts from the beginning.
CREATE TABLE IF NOT EXISTS maintenance_cursors (
    name TEXT PRIMARY KEY,
    published_at TEXT NOT NULL DEFAULT '',
    id TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
