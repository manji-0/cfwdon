-- Idempotency-Key replay for POST /api/v1/statuses (Mastodon keeps keys for 1 hour).
CREATE TABLE IF NOT EXISTS status_idempotency_keys (
    account_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    status_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (account_id, idempotency_key)
);

CREATE INDEX IF NOT EXISTS idx_status_idempotency_keys_created_at
    ON status_idempotency_keys (created_at);
