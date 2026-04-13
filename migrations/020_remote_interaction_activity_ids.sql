ALTER TABLE favourites
    ADD COLUMN ap_activity_id TEXT;

CREATE INDEX IF NOT EXISTS idx_favourites_ap_activity_id
    ON favourites (ap_activity_id);

ALTER TABLE reblogs
    ADD COLUMN ap_activity_id TEXT;

CREATE INDEX IF NOT EXISTS idx_reblogs_ap_activity_id
    ON reblogs (ap_activity_id);
