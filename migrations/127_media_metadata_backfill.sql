-- Objects uploaded before uploads were stripped of their metadata (EXIF with
-- its GPS position, XMP, a video's location tags) still carry it. Queue each
-- stored original, avatar, header and custom emoji once; the hourly cron strips
-- a page of them in place and deletes their rows, so the table empties and
-- stays empty (uploads from now on are stripped before they are stored).
-- `attempts` counts the runs that started on an object; one that never
-- finishes (it ends the invocation) sinks behind the rest and is given up.
CREATE TABLE IF NOT EXISTS media_metadata_backfill (
    object_key TEXT PRIMARY KEY,
    attempts INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_media_metadata_backfill_attempts
    ON media_metadata_backfill (attempts, object_key);

INSERT OR IGNORE INTO media_metadata_backfill (object_key)
SELECT object_key FROM media_attachments
UNION SELECT avatar_object_key FROM accounts WHERE avatar_object_key IS NOT NULL
UNION SELECT header_object_key FROM accounts WHERE header_object_key IS NOT NULL
UNION SELECT object_key FROM custom_emojis
UNION SELECT static_object_key FROM custom_emojis;
