CREATE UNIQUE INDEX IF NOT EXISTS idx_outbox_deliveries_activity_target_unique
    ON outbox_deliveries (activity_id, target_inbox)
    WHERE target_inbox IS NOT NULL;
