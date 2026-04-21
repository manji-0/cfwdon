CREATE TABLE IF NOT EXISTS oauth_app_accounts (
    oauth_app_id INTEGER NOT NULL,
    account_id TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (oauth_app_id, account_id),
    FOREIGN KEY (oauth_app_id) REFERENCES oauth_apps(id) ON DELETE CASCADE,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_oauth_app_accounts_account_id
    ON oauth_app_accounts (account_id);
