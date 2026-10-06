-- Mastodon's Status#replies_count: replies this server knows about, counting
-- only distributable (public / unlisted) replies. statuses.in_reply_to_id and
-- remote_statuses.in_reply_to_id may each point at either a local or a remote
-- status, so every trigger routes the change to whichever counts table owns
-- the parent. Decrements need no routing: the UPDATE simply misses the table
-- that does not hold the parent.
ALTER TABLE status_counts
    ADD COLUMN replies_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE remote_status_counts
    ADD COLUMN replies_count INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_remote_statuses_in_reply_to_id
    ON remote_statuses (in_reply_to_id);

INSERT INTO status_counts (status_id, replies_count, updated_at)
SELECT r.parent_id, COUNT(*), CURRENT_TIMESTAMP
FROM (
    SELECT in_reply_to_id AS parent_id
    FROM statuses
    WHERE in_reply_to_id IS NOT NULL
      AND visibility IN ('public', 'unlisted')
    UNION ALL
    SELECT in_reply_to_id AS parent_id
    FROM remote_statuses
    WHERE in_reply_to_id IS NOT NULL
      AND visibility IN ('public', 'unlisted')
) r
WHERE r.parent_id IN (SELECT id FROM statuses)
GROUP BY r.parent_id
ON CONFLICT(status_id) DO UPDATE SET
    replies_count = excluded.replies_count,
    updated_at = excluded.updated_at;

INSERT INTO remote_status_counts (remote_status_id, replies_count, updated_at)
SELECT r.parent_id, COUNT(*), CURRENT_TIMESTAMP
FROM (
    SELECT in_reply_to_id AS parent_id
    FROM statuses
    WHERE in_reply_to_id IS NOT NULL
      AND visibility IN ('public', 'unlisted')
    UNION ALL
    SELECT in_reply_to_id AS parent_id
    FROM remote_statuses
    WHERE in_reply_to_id IS NOT NULL
      AND visibility IN ('public', 'unlisted')
) r
WHERE r.parent_id IN (SELECT id FROM remote_statuses)
GROUP BY r.parent_id
ON CONFLICT(remote_status_id) DO UPDATE SET
    replies_count = excluded.replies_count,
    updated_at = excluded.updated_at;

CREATE TRIGGER IF NOT EXISTS trg_statuses_replies_count_insert
AFTER INSERT ON statuses
WHEN NEW.in_reply_to_id IS NOT NULL
 AND NEW.visibility IN ('public', 'unlisted')
BEGIN
    INSERT INTO status_counts (status_id, replies_count, updated_at)
    SELECT NEW.in_reply_to_id, 1, CURRENT_TIMESTAMP
    WHERE EXISTS (SELECT 1 FROM statuses WHERE id = NEW.in_reply_to_id)
    ON CONFLICT(status_id) DO UPDATE SET
        replies_count = replies_count + 1,
        updated_at = CURRENT_TIMESTAMP;
    INSERT INTO remote_status_counts (remote_status_id, replies_count, updated_at)
    SELECT NEW.in_reply_to_id, 1, CURRENT_TIMESTAMP
    WHERE EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.in_reply_to_id)
    ON CONFLICT(remote_status_id) DO UPDATE SET
        replies_count = replies_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_replies_count_update
AFTER UPDATE OF in_reply_to_id, visibility ON statuses
WHEN OLD.in_reply_to_id IS NOT NEW.in_reply_to_id
  OR OLD.visibility IS NOT NEW.visibility
BEGIN
    UPDATE status_counts
    SET replies_count = max(replies_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.in_reply_to_id
      AND OLD.visibility IN ('public', 'unlisted');
    UPDATE remote_status_counts
    SET replies_count = max(replies_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE remote_status_id = OLD.in_reply_to_id
      AND OLD.visibility IN ('public', 'unlisted');
    INSERT INTO status_counts (status_id, replies_count, updated_at)
    SELECT NEW.in_reply_to_id, 1, CURRENT_TIMESTAMP
    WHERE NEW.visibility IN ('public', 'unlisted')
      AND EXISTS (SELECT 1 FROM statuses WHERE id = NEW.in_reply_to_id)
    ON CONFLICT(status_id) DO UPDATE SET
        replies_count = replies_count + 1,
        updated_at = CURRENT_TIMESTAMP;
    INSERT INTO remote_status_counts (remote_status_id, replies_count, updated_at)
    SELECT NEW.in_reply_to_id, 1, CURRENT_TIMESTAMP
    WHERE NEW.visibility IN ('public', 'unlisted')
      AND EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.in_reply_to_id)
    ON CONFLICT(remote_status_id) DO UPDATE SET
        replies_count = replies_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_replies_count_delete
AFTER DELETE ON statuses
WHEN OLD.in_reply_to_id IS NOT NULL
 AND OLD.visibility IN ('public', 'unlisted')
BEGIN
    UPDATE status_counts
    SET replies_count = max(replies_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.in_reply_to_id;
    UPDATE remote_status_counts
    SET replies_count = max(replies_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE remote_status_id = OLD.in_reply_to_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_replies_count_insert
AFTER INSERT ON remote_statuses
WHEN NEW.in_reply_to_id IS NOT NULL
 AND NEW.visibility IN ('public', 'unlisted')
BEGIN
    INSERT INTO status_counts (status_id, replies_count, updated_at)
    SELECT NEW.in_reply_to_id, 1, CURRENT_TIMESTAMP
    WHERE EXISTS (SELECT 1 FROM statuses WHERE id = NEW.in_reply_to_id)
    ON CONFLICT(status_id) DO UPDATE SET
        replies_count = replies_count + 1,
        updated_at = CURRENT_TIMESTAMP;
    INSERT INTO remote_status_counts (remote_status_id, replies_count, updated_at)
    SELECT NEW.in_reply_to_id, 1, CURRENT_TIMESTAMP
    WHERE EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.in_reply_to_id)
    ON CONFLICT(remote_status_id) DO UPDATE SET
        replies_count = replies_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_replies_count_update
AFTER UPDATE OF in_reply_to_id, visibility ON remote_statuses
WHEN OLD.in_reply_to_id IS NOT NEW.in_reply_to_id
  OR OLD.visibility IS NOT NEW.visibility
BEGIN
    UPDATE status_counts
    SET replies_count = max(replies_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.in_reply_to_id
      AND OLD.visibility IN ('public', 'unlisted');
    UPDATE remote_status_counts
    SET replies_count = max(replies_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE remote_status_id = OLD.in_reply_to_id
      AND OLD.visibility IN ('public', 'unlisted');
    INSERT INTO status_counts (status_id, replies_count, updated_at)
    SELECT NEW.in_reply_to_id, 1, CURRENT_TIMESTAMP
    WHERE NEW.visibility IN ('public', 'unlisted')
      AND EXISTS (SELECT 1 FROM statuses WHERE id = NEW.in_reply_to_id)
    ON CONFLICT(status_id) DO UPDATE SET
        replies_count = replies_count + 1,
        updated_at = CURRENT_TIMESTAMP;
    INSERT INTO remote_status_counts (remote_status_id, replies_count, updated_at)
    SELECT NEW.in_reply_to_id, 1, CURRENT_TIMESTAMP
    WHERE NEW.visibility IN ('public', 'unlisted')
      AND EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.in_reply_to_id)
    ON CONFLICT(remote_status_id) DO UPDATE SET
        replies_count = replies_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_replies_count_delete
AFTER DELETE ON remote_statuses
WHEN OLD.in_reply_to_id IS NOT NULL
 AND OLD.visibility IN ('public', 'unlisted')
BEGIN
    UPDATE status_counts
    SET replies_count = max(replies_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.in_reply_to_id;
    UPDATE remote_status_counts
    SET replies_count = max(replies_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE remote_status_id = OLD.in_reply_to_id;
END;
