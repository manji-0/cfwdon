CREATE TABLE IF NOT EXISTS outbox_deliveries (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    status_id TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    activity_type TEXT NOT NULL,
    target_inbox TEXT,
    payload_json TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_attempt_at TEXT,
    next_attempt_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_outbox_deliveries_state_next_attempt
    ON outbox_deliveries (state, next_attempt_at, created_at);

CREATE INDEX IF NOT EXISTS idx_outbox_deliveries_status_id
    ON outbox_deliveries (status_id);
