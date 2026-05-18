CREATE INDEX IF NOT EXISTS idx_outbox_deliveries_state_target_next_attempt
    ON outbox_deliveries (state, target_inbox, next_attempt_at, created_at);

CREATE INDEX IF NOT EXISTS idx_followers_account_delivery_target
    ON followers (account_id, shared_inbox_uri, inbox_uri);
