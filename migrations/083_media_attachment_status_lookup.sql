CREATE INDEX IF NOT EXISTS idx_media_attachments_attached_status_created
    ON media_attachments (status_id, created_at ASC, id ASC)
    WHERE status_id IS NOT NULL;
