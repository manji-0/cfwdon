CREATE TABLE IF NOT EXISTS favourites (
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

CREATE INDEX IF NOT EXISTS idx_favourites_account_created_at
    ON favourites (account_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_favourites_status_id
    ON favourites (status_id);

CREATE INDEX IF NOT EXISTS idx_favourites_remote_status_id
    ON favourites (remote_status_id);
