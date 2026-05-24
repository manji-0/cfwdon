CREATE TABLE IF NOT EXISTS oauth_app_access_tokens (
    access_token TEXT PRIMARY KEY,
    oauth_app_id INTEGER NOT NULL,
    scopes_json TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (oauth_app_id) REFERENCES oauth_apps(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_oauth_app_access_tokens_oauth_app_id
    ON oauth_app_access_tokens (oauth_app_id);

CREATE INDEX IF NOT EXISTS idx_oauth_app_access_tokens_expires_at
    ON oauth_app_access_tokens (expires_at);
