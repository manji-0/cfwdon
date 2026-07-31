-- Superseded by idx_statuses_account_created_id (084) and
-- idx_remote_statuses_visibility_published_id (080).
DROP INDEX IF EXISTS idx_statuses_account_created_at;
DROP INDEX IF EXISTS idx_remote_statuses_visibility_published;
