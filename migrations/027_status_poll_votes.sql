CREATE TABLE status_poll_votes (
    id TEXT PRIMARY KEY,
    poll_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    option_position INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (poll_id, account_id, option_position)
);

CREATE INDEX idx_status_poll_votes_poll
    ON status_poll_votes (poll_id);

CREATE INDEX idx_status_poll_votes_account
    ON status_poll_votes (account_id);
