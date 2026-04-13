CREATE TABLE IF NOT EXISTS remote_actors (
    actor_uri TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    domain TEXT NOT NULL,
    inbox_uri TEXT NOT NULL,
    shared_inbox_uri TEXT,
    public_key_id TEXT NOT NULL,
    public_key_pem TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    summary_html TEXT NOT NULL DEFAULT '',
    profile_url TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_remote_actors_domain_username
    ON remote_actors (domain, username);
