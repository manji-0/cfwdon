CREATE TABLE IF NOT EXISTS bookmarks (
    account_id TEXT NOT NULL,
    status_id TEXT,
    remote_status_id TEXT,
    target_uri TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (account_id, target_uri),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE CASCADE,
    FOREIGN KEY (remote_status_id) REFERENCES remote_statuses(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_bookmarks_account_created_at
    ON bookmarks (account_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_bookmarks_status_id
    ON bookmarks (status_id);

CREATE INDEX IF NOT EXISTS idx_bookmarks_remote_status_id
    ON bookmarks (remote_status_id);
