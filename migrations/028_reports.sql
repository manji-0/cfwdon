CREATE TABLE reports (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    target_account_id TEXT NOT NULL,
    target_remote_actor_uri TEXT,
    comment TEXT NOT NULL DEFAULT '',
    category TEXT NOT NULL DEFAULT 'other',
    forward INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE report_statuses (
    report_id TEXT NOT NULL,
    status_id TEXT NOT NULL,
    PRIMARY KEY (report_id, status_id)
);

CREATE INDEX idx_reports_account_created_at
    ON reports (account_id, created_at DESC);

CREATE INDEX idx_reports_target_account_created_at
    ON reports (target_account_id, created_at DESC);
