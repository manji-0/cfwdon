CREATE TABLE IF NOT EXISTS remote_account_collections (
    id TEXT PRIMARY KEY,
    actor_uri TEXT NOT NULL,
    uri TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    language TEXT,
    sensitive INTEGER NOT NULL DEFAULT 0,
    discoverable INTEGER NOT NULL DEFAULT 1,
    tag_name TEXT,
    url TEXT,
    published_at TEXT,
    remote_updated_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (actor_uri) REFERENCES remote_actors(actor_uri) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_remote_account_collections_actor_created
    ON remote_account_collections (actor_uri, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_remote_account_collections_actor_discoverable_created
    ON remote_account_collections (actor_uri, discoverable, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS remote_account_collection_items (
    id TEXT PRIMARY KEY,
    collection_id TEXT NOT NULL,
    uri TEXT,
    target_actor_uri TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'accepted',
    feature_authorization TEXT,
    published_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (collection_id) REFERENCES remote_account_collections(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_remote_account_collection_items_unique
    ON remote_account_collection_items (collection_id, target_actor_uri);

CREATE INDEX IF NOT EXISTS idx_remote_account_collection_items_collection_created
    ON remote_account_collection_items (collection_id, created_at ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_remote_account_collection_items_target_created
    ON remote_account_collection_items (target_actor_uri, created_at DESC, id DESC);
