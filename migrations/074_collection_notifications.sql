CREATE TABLE IF NOT EXISTS collection_notifications (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    from_account_id TEXT NOT NULL,
    collection_id TEXT NOT NULL,
    collection_item_id TEXT,
    collection_item_key TEXT NOT NULL DEFAULT '',
    notification_type TEXT NOT NULL,
    filtered INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (from_account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (collection_id) REFERENCES account_collections(id) ON DELETE CASCADE,
    FOREIGN KEY (collection_item_id) REFERENCES account_collection_items(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_collection_notifications_unique
    ON collection_notifications (
        account_id,
        notification_type,
        collection_id,
        collection_item_key
    );

CREATE INDEX IF NOT EXISTS idx_collection_notifications_account_created
    ON collection_notifications (account_id, created_at DESC, id DESC);
