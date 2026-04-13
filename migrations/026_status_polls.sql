CREATE TABLE status_polls (
    id TEXT PRIMARY KEY,
    status_id TEXT NOT NULL UNIQUE,
    multiple INTEGER NOT NULL DEFAULT 0,
    hide_totals INTEGER NOT NULL DEFAULT 0,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE status_poll_options (
    id TEXT PRIMARY KEY,
    poll_id TEXT NOT NULL,
    title TEXT NOT NULL,
    position INTEGER NOT NULL,
    votes_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_status_poll_options_poll_position
    ON status_poll_options (poll_id, position);
