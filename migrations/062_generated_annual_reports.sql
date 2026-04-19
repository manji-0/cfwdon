CREATE TABLE IF NOT EXISTS generated_annual_reports (
    account_id TEXT NOT NULL,
    year INTEGER NOT NULL,
    data_json TEXT NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1,
    share_key TEXT,
    viewed_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (account_id, year),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_generated_annual_reports_account_pending
    ON generated_annual_reports (account_id, viewed_at, year DESC);
