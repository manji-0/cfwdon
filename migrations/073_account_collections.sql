CREATE TABLE IF NOT EXISTS account_collections (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    language TEXT,
    sensitive INTEGER NOT NULL DEFAULT 0,
    discoverable INTEGER NOT NULL DEFAULT 1,
    tag_name TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_account_collections_account_created
    ON account_collections (account_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_account_collections_account_discoverable_created
    ON account_collections (account_id, discoverable, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS account_collection_items (
    id TEXT PRIMARY KEY,
    collection_id TEXT NOT NULL,
    target_account_ref TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'accepted',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (collection_id) REFERENCES account_collections(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_account_collection_items_unique
    ON account_collection_items (collection_id, target_account_ref);

CREATE INDEX IF NOT EXISTS idx_account_collection_items_collection_created
    ON account_collection_items (collection_id, created_at ASC, id ASC);
