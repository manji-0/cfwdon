ALTER TABLE status_poll_votes
    ADD COLUMN activity_uri TEXT;

CREATE UNIQUE INDEX idx_status_poll_votes_activity_uri
    ON status_poll_votes (activity_uri)
    WHERE activity_uri IS NOT NULL;
