CREATE TABLE IF NOT EXISTS oauth_access_tokens (
    access_token TEXT PRIMARY KEY,
    oauth_app_id INTEGER NOT NULL,
    account_id TEXT NOT NULL,
    scopes_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (oauth_app_id) REFERENCES oauth_apps(id) ON DELETE CASCADE,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_oauth_access_tokens_account_id
    ON oauth_access_tokens (account_id);

CREATE INDEX IF NOT EXISTS idx_oauth_access_tokens_oauth_app_id
    ON oauth_access_tokens (oauth_app_id);
