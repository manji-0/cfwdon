ALTER TABLE remote_actors
    ADD COLUMN followers_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE remote_actors
    ADD COLUMN following_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE remote_actors
    ADD COLUMN statuses_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE remote_actors
    ADD COLUMN social_counts_updated_at TEXT;
