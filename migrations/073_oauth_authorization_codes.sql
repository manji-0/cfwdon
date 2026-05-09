CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    code TEXT PRIMARY KEY,
    oauth_app_id INTEGER NOT NULL,
    account_id TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    scopes_json TEXT NOT NULL,
    code_challenge TEXT,
    code_challenge_method TEXT,
    expires_at INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (oauth_app_id) REFERENCES oauth_apps(id) ON DELETE CASCADE,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_oauth_authorization_codes_expires_at
    ON oauth_authorization_codes (expires_at);

CREATE TABLE IF NOT EXISTS account_password_credentials (
    account_id TEXT PRIMARY KEY,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
