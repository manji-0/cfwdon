CREATE TABLE IF NOT EXISTS reblogs (
    account_id TEXT NOT NULL,
    status_id TEXT,
    remote_status_id TEXT,
    target_uri TEXT NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'public',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (account_id, target_uri),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE CASCADE,
    FOREIGN KEY (remote_status_id) REFERENCES remote_statuses(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_reblogs_account_created_at
    ON reblogs (account_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_reblogs_status_id
    ON reblogs (status_id);

CREATE INDEX IF NOT EXISTS idx_reblogs_remote_status_id
    ON reblogs (remote_status_id);
