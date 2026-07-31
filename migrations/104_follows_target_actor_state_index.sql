CREATE INDEX IF NOT EXISTS idx_follows_target_actor_state
    ON follows (target_actor_uri, state, created_at DESC);
