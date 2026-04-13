CREATE TABLE IF NOT EXISTS outbound_activities (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    activity_id TEXT NOT NULL UNIQUE,
    activity_type TEXT NOT NULL,
    target_actor_uri TEXT,
    target_inbox TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_attempt_at TEXT,
    next_attempt_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_outbound_activities_state_next_attempt
    ON outbound_activities (state, next_attempt_at, created_at);

CREATE INDEX IF NOT EXISTS idx_outbound_activities_account_type
    ON outbound_activities (account_id, activity_type, created_at DESC);
