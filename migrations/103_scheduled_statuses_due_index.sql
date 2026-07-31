CREATE INDEX IF NOT EXISTS idx_scheduled_statuses_due
    ON scheduled_statuses (scheduled_at, id);
