CREATE TABLE IF NOT EXISTS notification_dismissals (
    account_id TEXT NOT NULL,
    notification_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (account_id, notification_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_notification_dismissals_account_created
    ON notification_dismissals (account_id, created_at DESC);

CREATE TABLE IF NOT EXISTS notification_clear_markers (
    account_id TEXT PRIMARY KEY,
    cleared_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
