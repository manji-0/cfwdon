CREATE TABLE IF NOT EXISTS follow_requests (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    requester_account_id TEXT,
    requester_actor_uri TEXT,
    requester_inbox_uri TEXT,
    requester_shared_inbox_uri TEXT,
    follow_activity_id TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (requester_account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    CHECK (
        (requester_account_id IS NOT NULL AND requester_actor_uri IS NULL)
        OR (requester_account_id IS NULL AND requester_actor_uri IS NOT NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_follow_requests_local_unique
    ON follow_requests (account_id, requester_account_id)
    WHERE requester_account_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_follow_requests_remote_unique
    ON follow_requests (account_id, requester_actor_uri)
    WHERE requester_actor_uri IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_follow_requests_account_created
    ON follow_requests (account_id, created_at DESC);
