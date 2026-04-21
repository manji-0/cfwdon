ALTER TABLE statuses
ADD COLUMN application_id INTEGER REFERENCES oauth_apps(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_statuses_application_id_created_at
    ON statuses (application_id, created_at DESC);
