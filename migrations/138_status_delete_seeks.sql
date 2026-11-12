-- Deleting a local status read rows it never needed, and D1 bills every row
-- read:
-- * The account-stats triggers recomputed `last_status_at` as
--   `MAX(substr(created_at, 1, 10))`, an expression no index holds, so each
--   delete read all of the author's statuses. `substr(MAX(created_at), 1, 10)`
--   is the same day (timestamps sort by their date prefix) and is one seek on
--   `idx_statuses_account_created_id`.
-- * The `ON DELETE CASCADE` lookups on `status_pins.status_id` and
--   `thread_mutes.thread_root_status_id` scanned both tables: neither column
--   leads an index. Both tables are written rarely, so the index rows they add
--   cost little.

DROP TRIGGER IF EXISTS trg_statuses_account_stats_delete;
CREATE TRIGGER trg_statuses_account_stats_delete
AFTER DELETE ON statuses
BEGIN
    UPDATE account_stats
    SET statuses_count = max(statuses_count - 1, 0),
        last_status_at = (
            SELECT substr(MAX(created_at), 1, 10)
            FROM statuses
            WHERE account_id = OLD.account_id
        ),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.account_id;
END;

DROP TRIGGER IF EXISTS trg_statuses_account_stats_update_old;
CREATE TRIGGER trg_statuses_account_stats_update_old
AFTER UPDATE ON statuses
WHEN NEW.account_id <> OLD.account_id
BEGIN
    UPDATE account_stats
    SET statuses_count = max(statuses_count - 1, 0),
        last_status_at = (
            SELECT substr(MAX(created_at), 1, 10)
            FROM statuses
            WHERE account_id = OLD.account_id
        ),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.account_id;
END;

CREATE INDEX IF NOT EXISTS idx_status_pins_status_id ON status_pins (status_id);
CREATE INDEX IF NOT EXISTS idx_thread_mutes_root ON thread_mutes (thread_root_status_id);
