CREATE TABLE IF NOT EXISTS blocks (
    blocker_account_id TEXT NOT NULL,
    target_account_id TEXT,
    target_actor_uri TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (blocker_account_id, target_actor_uri),
    FOREIGN KEY (blocker_account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_blocks_target_account_id
    ON blocks (target_account_id);
