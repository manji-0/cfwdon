CREATE TABLE IF NOT EXISTS pending_email_confirmations (
    account_id TEXT PRIMARY KEY,
    oauth_app_id INTEGER NOT NULL,
    pending_email TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (oauth_app_id) REFERENCES oauth_apps(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pending_email_confirmations_oauth_app_id
    ON pending_email_confirmations (oauth_app_id);
