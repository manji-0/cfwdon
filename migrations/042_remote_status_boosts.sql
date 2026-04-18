ALTER TABLE remote_statuses
    ADD COLUMN boost_of_uri TEXT;

CREATE INDEX IF NOT EXISTS idx_remote_statuses_boost_of_uri
    ON remote_statuses (boost_of_uri);
