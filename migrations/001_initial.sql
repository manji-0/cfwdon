CREATE TABLE IF NOT EXISTS instance_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    domain TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    access_email TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    bio_html TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS statuses (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    ap_id TEXT UNIQUE,
    in_reply_to_id TEXT,
    content_html TEXT NOT NULL,
    visibility TEXT NOT NULL,
    sensitive INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (in_reply_to_id) REFERENCES statuses(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_statuses_account_created_at
    ON statuses (account_id, created_at DESC);

CREATE TABLE IF NOT EXISTS media_attachments (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    status_id TEXT,
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_media_attachments_account_created_at
    ON media_attachments (account_id, created_at DESC);
