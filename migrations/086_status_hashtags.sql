CREATE TABLE IF NOT EXISTS status_hashtags (
    status_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    account_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (status_id, tag),
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE CASCADE,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_status_hashtags_tag_created
    ON status_hashtags (tag, created_at DESC, status_id);

CREATE INDEX IF NOT EXISTS idx_status_hashtags_account_created
    ON status_hashtags (account_id, created_at DESC);

CREATE TABLE IF NOT EXISTS remote_status_hashtags (
    status_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    actor_uri TEXT NOT NULL,
    published_at TEXT NOT NULL,
    PRIMARY KEY (status_id, tag),
    FOREIGN KEY (actor_uri) REFERENCES remote_actors(actor_uri) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_remote_status_hashtags_tag_published
    ON remote_status_hashtags (tag, published_at DESC, status_id);

CREATE INDEX IF NOT EXISTS idx_remote_status_hashtags_actor_published
    ON remote_status_hashtags (actor_uri, published_at DESC);

CREATE TRIGGER IF NOT EXISTS trg_remote_status_hashtags_status_insert
BEFORE INSERT ON remote_status_hashtags
WHEN NOT EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_hashtags.status_id must reference remote_statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_status_hashtags_status_update
BEFORE UPDATE OF status_id ON remote_status_hashtags
WHEN NOT EXISTS (SELECT 1 FROM remote_statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'remote_status_hashtags.status_id must reference remote_statuses.id');
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_delete_hashtags
AFTER DELETE ON remote_statuses
BEGIN
    DELETE FROM remote_status_hashtags
    WHERE status_id = OLD.id;
END;
