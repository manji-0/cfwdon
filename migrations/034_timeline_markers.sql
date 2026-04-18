CREATE TABLE IF NOT EXISTS timeline_markers (
    account_id TEXT NOT NULL,
    scope TEXT NOT NULL,
    last_read_id TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (account_id, scope),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_timeline_markers_account_updated
    ON timeline_markers (account_id, updated_at DESC);
