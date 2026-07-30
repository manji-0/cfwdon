-- Profile Update fan-out reuses outbox_deliveries without a status row.
-- Keep the statuses FK for status-bound rows, but allow NULL status_id.
PRAGMA foreign_keys = OFF;

CREATE TABLE outbox_deliveries_new (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    status_id TEXT,
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

INSERT INTO outbox_deliveries_new (
    id,
    account_id,
    status_id,
    activity_id,
    activity_type,
    target_inbox,
    payload_json,
    state,
    attempt_count,
    last_attempt_at,
    next_attempt_at,
    created_at,
    updated_at
)
SELECT
    id,
    account_id,
    status_id,
    activity_id,
    activity_type,
    target_inbox,
    payload_json,
    state,
    attempt_count,
    last_attempt_at,
    next_attempt_at,
    created_at,
    updated_at
FROM outbox_deliveries;

DROP TABLE outbox_deliveries;
ALTER TABLE outbox_deliveries_new RENAME TO outbox_deliveries;

CREATE INDEX IF NOT EXISTS idx_outbox_deliveries_state_next_attempt
    ON outbox_deliveries (state, next_attempt_at, created_at);

CREATE INDEX IF NOT EXISTS idx_outbox_deliveries_status_id
    ON outbox_deliveries (status_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_outbox_deliveries_activity_target_unique
    ON outbox_deliveries (activity_id, target_inbox);

CREATE INDEX IF NOT EXISTS idx_outbox_deliveries_state_target_next_attempt
    ON outbox_deliveries (state, target_inbox, next_attempt_at, created_at);

PRAGMA foreign_keys = ON;
