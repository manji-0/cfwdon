CREATE TABLE IF NOT EXISTS thread_mutes (
    account_id TEXT NOT NULL,
    thread_root_status_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (account_id, thread_root_status_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (thread_root_status_id) REFERENCES statuses(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_thread_mutes_account_created
    ON thread_mutes (account_id, created_at DESC);
