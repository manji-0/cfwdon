CREATE TABLE IF NOT EXISTS account_lists (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    title TEXT NOT NULL,
    replies_policy TEXT NOT NULL DEFAULT 'list',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_account_lists_account_created
    ON account_lists (account_id, created_at DESC);

CREATE TABLE IF NOT EXISTS account_list_memberships (
    list_id TEXT NOT NULL,
    target_account_ref TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (list_id, target_account_ref),
    FOREIGN KEY (list_id) REFERENCES account_lists(id) ON DELETE CASCADE
);
