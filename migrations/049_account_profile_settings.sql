CREATE TABLE IF NOT EXISTS account_profile_settings (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    hide_collections INTEGER,
    indexable INTEGER NOT NULL DEFAULT 1,
    show_media INTEGER NOT NULL DEFAULT 1,
    show_media_replies INTEGER NOT NULL DEFAULT 1,
    show_featured INTEGER NOT NULL DEFAULT 1,
    avatar_description TEXT NOT NULL DEFAULT '',
    header_description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
