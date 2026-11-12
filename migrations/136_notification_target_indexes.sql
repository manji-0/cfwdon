-- Notifications are derived on every read, so each collector must seek the
-- viewer's own rows newest first instead of walking every status the viewer
-- wrote (or every status on the instance). Record, when each row is written,
-- the local account it notifies, and index that account with the row's time.
-- NULL means the row notifies nobody: a favourite or boost of a remote status
-- or of one's own status, a quote of a remote or of one's own status.

-- Favourites and boosts. The `(created_at DESC, status_id, ...)` indexes from
-- 088 served only the notification collectors; the new indexes replace them.
ALTER TABLE favourites ADD COLUMN notified_account_id TEXT;
UPDATE favourites
SET notified_account_id = (
    SELECT s.account_id FROM statuses s
    WHERE s.id = favourites.status_id AND s.account_id <> favourites.account_id
)
WHERE status_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_favourites_notified_created
    ON favourites (notified_account_id, created_at DESC)
    WHERE notified_account_id IS NOT NULL;
DROP INDEX IF EXISTS idx_favourites_created_status_account;

ALTER TABLE reblogs ADD COLUMN notified_account_id TEXT;
UPDATE reblogs
SET notified_account_id = (
    SELECT s.account_id FROM statuses s
    WHERE s.id = reblogs.status_id AND s.account_id <> reblogs.account_id
)
WHERE status_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_reblogs_notified_created
    ON reblogs (notified_account_id, created_at DESC)
    WHERE notified_account_id IS NOT NULL;
DROP INDEX IF EXISTS idx_reblogs_created_status_account;

ALTER TABLE remote_favourites ADD COLUMN notified_account_id TEXT;
UPDATE remote_favourites
SET notified_account_id = (
    SELECT s.account_id FROM statuses s WHERE s.id = remote_favourites.status_id
);
CREATE INDEX IF NOT EXISTS idx_remote_favourites_notified_created
    ON remote_favourites (notified_account_id, created_at DESC)
    WHERE notified_account_id IS NOT NULL;
DROP INDEX IF EXISTS idx_remote_favourites_created_status_actor;

ALTER TABLE remote_reblogs ADD COLUMN notified_account_id TEXT;
UPDATE remote_reblogs
SET notified_account_id = (
    SELECT s.account_id FROM statuses s WHERE s.id = remote_reblogs.status_id
);
CREATE INDEX IF NOT EXISTS idx_remote_reblogs_notified_created
    ON remote_reblogs (notified_account_id, created_at DESC)
    WHERE notified_account_id IS NOT NULL;
DROP INDEX IF EXISTS idx_remote_reblogs_created_status_actor;

-- Quotes of a local status. The collectors still join the quoted status, so
-- a deleted quoted status stops notifying as before.
ALTER TABLE statuses ADD COLUMN quoted_account_id TEXT;
UPDATE statuses
SET quoted_account_id = (
    SELECT target.account_id FROM statuses target
    WHERE target.ap_id = statuses.quote_of_uri AND target.account_id <> statuses.account_id
)
WHERE quote_of_uri IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_statuses_quoted_account_created
    ON statuses (quoted_account_id, created_at DESC)
    WHERE quoted_account_id IS NOT NULL;

ALTER TABLE remote_statuses ADD COLUMN quoted_account_id TEXT;
UPDATE remote_statuses
SET quoted_account_id = (
    SELECT target.account_id FROM statuses target WHERE target.ap_id = remote_statuses.quote_of_uri
)
WHERE quote_of_uri IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_remote_statuses_quoted_account_published
    ON remote_statuses (quoted_account_id, published_at DESC, id DESC)
    WHERE quoted_account_id IS NOT NULL;

-- The viewer's own quote posts, whose quoted remote status may be edited.
CREATE INDEX IF NOT EXISTS idx_statuses_account_quoting
    ON statuses (account_id, quote_of_uri)
    WHERE quote_of_uri IS NOT NULL;

-- Polls by their author; polls the viewer voted in seek `status_poll_votes`.
ALTER TABLE status_polls ADD COLUMN account_id TEXT;
UPDATE status_polls
SET account_id = (SELECT s.account_id FROM statuses s WHERE s.id = status_polls.status_id);
CREATE INDEX IF NOT EXISTS idx_status_polls_account_expires
    ON status_polls (account_id, expires_at DESC)
    WHERE account_id IS NOT NULL;

-- Remote follows newest first, instead of every remote follower of the viewer.
CREATE INDEX IF NOT EXISTS idx_followers_account_created
    ON followers (account_id, created_at DESC);

-- Mention notifications seek `status_mentions` by the mentioned account.
-- Statuses written before those rows existed (112) have none; queue the ones
-- that may mention someone, and the hourly cron resolves them a page at a
-- time with the same code that writes the rows for new statuses.
CREATE TABLE IF NOT EXISTS status_mentions_backfill (
    status_id TEXT PRIMARY KEY
) WITHOUT ROWID;
INSERT OR IGNORE INTO status_mentions_backfill (status_id)
SELECT s.id FROM statuses s
WHERE s.boost_of_uri IS NULL
  AND instr(s.text_content, '@') > 0
  AND NOT EXISTS (SELECT 1 FROM status_mentions m WHERE m.status_id = s.id);
