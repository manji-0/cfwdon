CREATE INDEX IF NOT EXISTS idx_statuses_visibility_created_id
    ON statuses (visibility, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_statuses_in_reply_to_id
    ON statuses (in_reply_to_id);

CREATE INDEX IF NOT EXISTS idx_media_attachments_status_created
    ON media_attachments (status_id, created_at ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_remote_statuses_visibility_published_id
    ON remote_statuses (visibility, published_at DESC, id DESC);
