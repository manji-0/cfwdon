ALTER TABLE outbox_deliveries ADD COLUMN last_error TEXT;
ALTER TABLE outbound_activities ADD COLUMN last_error TEXT;

CREATE TABLE IF NOT EXISTS instance_domain_blocks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    domain TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by_account_id TEXT,
    FOREIGN KEY (created_by_account_id) REFERENCES accounts(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_instance_domain_blocks_created
    ON instance_domain_blocks (created_at DESC);
