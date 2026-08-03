CREATE TABLE IF NOT EXISTS federation_relays (
    id TEXT PRIMARY KEY,
    inbox_url TEXT NOT NULL UNIQUE,
    actor_uri TEXT,
    follow_activity_id TEXT,
    signing_account_id TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'idle',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_federation_relays_state
    ON federation_relays (state, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_federation_relays_follow_activity_id
    ON federation_relays (follow_activity_id)
    WHERE follow_activity_id IS NOT NULL;
