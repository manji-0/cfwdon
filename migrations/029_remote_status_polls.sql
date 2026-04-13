CREATE TABLE remote_status_polls (
    id TEXT PRIMARY KEY,
    status_id TEXT NOT NULL UNIQUE,
    multiple INTEGER NOT NULL DEFAULT 0,
    expires_at TEXT,
    voters_count INTEGER,
    votes_count INTEGER NOT NULL DEFAULT 0,
    expired INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE remote_status_poll_options (
    poll_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    title TEXT NOT NULL,
    votes_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (poll_id, position)
);

CREATE INDEX idx_remote_status_polls_status_id
    ON remote_status_polls (status_id);
