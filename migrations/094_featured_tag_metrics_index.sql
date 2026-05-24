CREATE INDEX IF NOT EXISTS idx_status_hashtags_account_tag_created
    ON status_hashtags (account_id, tag, created_at DESC);
