CREATE TABLE IF NOT EXISTS account_social_metadata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL,
    target_account_id TEXT,
    target_actor_uri TEXT NOT NULL,
    endorsed INTEGER NOT NULL DEFAULT 0,
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (account_id, target_actor_uri),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_account_social_metadata_endorsed
    ON account_social_metadata (account_id, endorsed, id DESC);
