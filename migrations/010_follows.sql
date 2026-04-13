CREATE TABLE IF NOT EXISTS follows (
    id TEXT PRIMARY KEY,
    follower_account_id TEXT NOT NULL,
    target_account_id TEXT,
    target_actor_uri TEXT NOT NULL,
    target_inbox_uri TEXT,
    target_shared_inbox_uri TEXT,
    follow_activity_id TEXT,
    state TEXT NOT NULL DEFAULT 'accepted',
    show_reblogs INTEGER NOT NULL DEFAULT 1,
    notify INTEGER NOT NULL DEFAULT 0,
    languages_json TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (follower_account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_follows_follower_target_unique
    ON follows (follower_account_id, target_actor_uri);

CREATE UNIQUE INDEX IF NOT EXISTS idx_follows_activity_id_unique
    ON follows (follow_activity_id)
    WHERE follow_activity_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_follows_follower_state
    ON follows (follower_account_id, state, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_follows_target_state
    ON follows (target_account_id, state, created_at DESC);
