-- The hourly retention prune deletes terminal rows by age. Index the terminal
-- states by `updated_at` so each batch is a range seek, not a walk of every
-- delivered row.
CREATE INDEX IF NOT EXISTS idx_outbox_deliveries_state_updated
    ON outbox_deliveries (state, updated_at);

CREATE INDEX IF NOT EXISTS idx_outbound_activities_state_updated
    ON outbound_activities (state, updated_at);
