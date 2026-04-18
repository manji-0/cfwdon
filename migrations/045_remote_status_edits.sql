CREATE TABLE IF NOT EXISTS remote_status_edits (
    id TEXT PRIMARY KEY,
    status_id TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (status_id) REFERENCES remote_statuses(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_remote_status_edits_status_created
    ON remote_status_edits (status_id, created_at DESC, id DESC);
