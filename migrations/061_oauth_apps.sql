CREATE TABLE IF NOT EXISTS oauth_apps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    website TEXT,
    scopes_json TEXT NOT NULL,
    redirect_uris_json TEXT NOT NULL,
    redirect_uri_legacy TEXT NOT NULL,
    client_id TEXT NOT NULL UNIQUE,
    client_secret TEXT NOT NULL,
    client_secret_expires_at INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
