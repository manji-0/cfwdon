CREATE TABLE IF NOT EXISTS status_mentions (
    status_id TEXT NOT NULL,
    mention_key TEXT NOT NULL,
    account_id TEXT,
    actor_uri TEXT,
    username TEXT NOT NULL,
    acct TEXT NOT NULL,
    url TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (status_id, mention_key),
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_status_mentions_account_created
    ON status_mentions (account_id, created_at DESC)
    WHERE account_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_status_mentions_actor_created
    ON status_mentions (actor_uri, created_at DESC)
    WHERE actor_uri IS NOT NULL;

CREATE TABLE IF NOT EXISTS remote_status_mentions (
    status_id TEXT NOT NULL,
    mention_key TEXT NOT NULL,
    account_id TEXT,
    actor_uri TEXT,
    username TEXT NOT NULL,
    acct TEXT NOT NULL,
    url TEXT NOT NULL,
    published_at TEXT NOT NULL,
    PRIMARY KEY (status_id, mention_key)
);

CREATE INDEX IF NOT EXISTS idx_remote_status_mentions_account_published
    ON remote_status_mentions (account_id, published_at DESC)
    WHERE account_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_remote_status_mentions_actor_published
    ON remote_status_mentions (actor_uri, published_at DESC)
    WHERE actor_uri IS NOT NULL;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_mentions_status_insert
BEFORE INSERT ON remote_status_mentions
WHEN NOT EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_mentions.status_id must reference remote_statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_delete_mentions
AFTER DELETE ON remote_statuses
BEGIN
    DELETE FROM remote_status_mentions
    WHERE status_id = OLD.id;
END;
