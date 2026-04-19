CREATE TABLE IF NOT EXISTS notification_policies (
    account_id TEXT PRIMARY KEY,
    for_not_following TEXT NOT NULL DEFAULT 'accept',
    for_not_followers TEXT NOT NULL DEFAULT 'accept',
    for_new_accounts TEXT NOT NULL DEFAULT 'accept',
    for_private_mentions TEXT NOT NULL DEFAULT 'drop',
    for_limited_accounts TEXT NOT NULL DEFAULT 'filter',
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
