ALTER TABLE reports ADD COLUMN action_taken INTEGER NOT NULL DEFAULT 0;
ALTER TABLE reports ADD COLUMN action_taken_at TEXT;
ALTER TABLE reports ADD COLUMN action_taken_by_account_id TEXT;

CREATE INDEX IF NOT EXISTS idx_reports_action_taken_created_at
    ON reports (action_taken, created_at DESC);
