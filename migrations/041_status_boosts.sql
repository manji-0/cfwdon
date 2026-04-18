ALTER TABLE statuses
    ADD COLUMN boost_of_uri TEXT;

CREATE INDEX IF NOT EXISTS idx_statuses_boost_of_uri
    ON statuses (boost_of_uri);
