ALTER TABLE remote_status_poll_votes
    ADD COLUMN option_title TEXT;

UPDATE remote_status_poll_votes
SET option_title = (
    SELECT o.title
    FROM remote_status_poll_options o
    WHERE o.poll_id = remote_status_poll_votes.poll_id
      AND o.position = remote_status_poll_votes.option_position
    LIMIT 1
)
WHERE option_title IS NULL;
