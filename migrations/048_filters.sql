CREATE TABLE IF NOT EXISTS filters (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    title TEXT NOT NULL,
    context_csv TEXT NOT NULL,
    expires_at TEXT,
    filter_action TEXT NOT NULL DEFAULT 'warn',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_filters_account_created
    ON filters (account_id, created_at DESC);

CREATE TABLE IF NOT EXISTS filter_keywords (
    id TEXT PRIMARY KEY,
    filter_id TEXT NOT NULL,
    keyword TEXT NOT NULL,
    whole_word INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (filter_id) REFERENCES filters(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_filter_keywords_filter_created
    ON filter_keywords (filter_id, created_at ASC);

CREATE TABLE IF NOT EXISTS filter_statuses (
    id TEXT PRIMARY KEY,
    filter_id TEXT NOT NULL,
    status_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (filter_id) REFERENCES filters(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_filter_statuses_filter_status
    ON filter_statuses (filter_id, status_id);
