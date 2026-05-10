CREATE INDEX IF NOT EXISTS idx_statuses_account_created_id
    ON statuses (account_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_follows_follower_state_target_account
    ON follows (follower_account_id, state, target_account_id)
    WHERE target_account_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_filters_account_created_id
    ON filters (account_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_follows_follower_state_target_actor
    ON follows (follower_account_id, state, target_actor_uri);
