CREATE TABLE IF NOT EXISTS followers (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    actor_uri TEXT NOT NULL,
    inbox_uri TEXT NOT NULL,
    shared_inbox_uri TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_followers_account_inbox
    ON followers (account_id, inbox_uri);

CREATE UNIQUE INDEX IF NOT EXISTS idx_followers_account_actor_unique
    ON followers (account_id, actor_uri);
