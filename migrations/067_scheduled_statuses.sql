CREATE TABLE IF NOT EXISTS scheduled_statuses (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    text_content TEXT NOT NULL DEFAULT '',
    visibility TEXT NOT NULL,
    spoiler_text TEXT NOT NULL DEFAULT '',
    sensitive INTEGER NOT NULL DEFAULT 0,
    language TEXT,
    quote_approval_policy TEXT,
    in_reply_to_id TEXT,
    media_ids_json TEXT NOT NULL DEFAULT '[]',
    poll_json TEXT,
    idempotency_key TEXT,
    application_id INTEGER,
    quote_of_uri TEXT,
    scheduled_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (in_reply_to_id) REFERENCES statuses(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_scheduled_statuses_account_schedule
    ON scheduled_statuses (account_id, scheduled_at, id);
