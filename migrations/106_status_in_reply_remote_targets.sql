-- in_reply_to_id may reference remote_statuses.id as well as local statuses.id.
PRAGMA foreign_keys = OFF;

CREATE TABLE statuses_new (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    ap_id TEXT UNIQUE,
    in_reply_to_id TEXT,
    content_html TEXT NOT NULL,
    visibility TEXT NOT NULL,
    sensitive INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    text_content TEXT NOT NULL DEFAULT '',
    spoiler_text TEXT NOT NULL DEFAULT '',
    language TEXT,
    boost_of_uri TEXT,
    quote_of_uri TEXT,
    quote_approval_policy TEXT,
    quote_state TEXT NOT NULL DEFAULT 'accepted',
    application_id INTEGER REFERENCES oauth_apps(id) ON DELETE SET NULL,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

INSERT INTO statuses_new (
    id,
    account_id,
    ap_id,
    in_reply_to_id,
    content_html,
    visibility,
    sensitive,
    created_at,
    updated_at,
    text_content,
    spoiler_text,
    language,
    boost_of_uri,
    quote_of_uri,
    quote_approval_policy,
    quote_state,
    application_id
)
SELECT
    id,
    account_id,
    ap_id,
    in_reply_to_id,
    content_html,
    visibility,
    sensitive,
    created_at,
    updated_at,
    text_content,
    spoiler_text,
    language,
    boost_of_uri,
    quote_of_uri,
    quote_approval_policy,
    quote_state,
    application_id
FROM statuses;

DROP TRIGGER IF EXISTS trg_status_polls_status_insert;
DROP TRIGGER IF EXISTS trg_status_polls_status_update;

DROP TABLE statuses;
ALTER TABLE statuses_new RENAME TO statuses;

CREATE INDEX IF NOT EXISTS idx_statuses_boost_of_uri
    ON statuses (boost_of_uri);

CREATE INDEX IF NOT EXISTS idx_statuses_quote_of_uri
    ON statuses (quote_of_uri);

CREATE INDEX IF NOT EXISTS idx_statuses_quote_of_uri_state
    ON statuses (quote_of_uri, quote_state);

CREATE INDEX IF NOT EXISTS idx_statuses_application_id_created_at
    ON statuses (application_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_statuses_visibility_created_id
    ON statuses (visibility, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_statuses_in_reply_to_id
    ON statuses (in_reply_to_id);

CREATE INDEX IF NOT EXISTS idx_statuses_account_created_id
    ON statuses (account_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_statuses_created_id
    ON statuses (created_at DESC, id DESC);

CREATE TRIGGER trg_statuses_account_stats_insert
AFTER INSERT ON statuses
BEGIN
    INSERT INTO account_stats (account_id, statuses_count, last_status_at, updated_at)
    VALUES (NEW.account_id, 1, substr(NEW.created_at, 1, 10), CURRENT_TIMESTAMP)
    ON CONFLICT(account_id) DO UPDATE SET
        statuses_count = statuses_count + 1,
        last_status_at = max(
            coalesce(last_status_at, ''),
            substr(NEW.created_at, 1, 10)
        ),
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER trg_statuses_account_stats_delete
AFTER DELETE ON statuses
BEGIN
    UPDATE account_stats
    SET statuses_count = max(statuses_count - 1, 0),
        last_status_at = (
            SELECT MAX(substr(created_at, 1, 10))
            FROM statuses
            WHERE account_id = OLD.account_id
        ),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.account_id;
END;

CREATE TRIGGER trg_statuses_account_stats_update_old
AFTER UPDATE ON statuses
WHEN NEW.account_id <> OLD.account_id
BEGIN
    UPDATE account_stats
    SET statuses_count = max(statuses_count - 1, 0),
        last_status_at = (
            SELECT MAX(substr(created_at, 1, 10))
            FROM statuses
            WHERE account_id = OLD.account_id
        ),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.account_id;
END;

CREATE TRIGGER trg_statuses_account_stats_update_new
AFTER UPDATE ON statuses
WHEN NEW.account_id <> OLD.account_id
BEGIN
    INSERT INTO account_stats (account_id, statuses_count, last_status_at, updated_at)
    VALUES (NEW.account_id, 1, substr(NEW.created_at, 1, 10), CURRENT_TIMESTAMP)
    ON CONFLICT(account_id) DO UPDATE SET
        statuses_count = statuses_count + 1,
        last_status_at = max(
            coalesce(last_status_at, ''),
            substr(NEW.created_at, 1, 10)
        ),
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER trg_statuses_visibility_insert
BEFORE INSERT ON statuses
WHEN NEW.visibility NOT IN ('public', 'unlisted', 'private', 'direct')
BEGIN
    SELECT RAISE(ABORT, 'statuses.visibility must be public, unlisted, private, or direct');
END;

CREATE TRIGGER trg_statuses_visibility_update
BEFORE UPDATE OF visibility ON statuses
WHEN NEW.visibility NOT IN ('public', 'unlisted', 'private', 'direct')
BEGIN
    SELECT RAISE(ABORT, 'statuses.visibility must be public, unlisted, private, or direct');
END;

CREATE TRIGGER trg_statuses_quote_policy_insert
BEFORE INSERT ON statuses
WHEN NEW.quote_approval_policy NOT IN ('public', 'followers', 'nobody')
BEGIN
    SELECT RAISE(ABORT, 'statuses.quote_approval_policy must be public, followers, or nobody');
END;

CREATE TRIGGER trg_statuses_quote_policy_update
BEFORE UPDATE OF quote_approval_policy ON statuses
WHEN NEW.quote_approval_policy NOT IN ('public', 'followers', 'nobody')
BEGIN
    SELECT RAISE(ABORT, 'statuses.quote_approval_policy must be public, followers, or nobody');
END;

CREATE TRIGGER trg_statuses_quote_state_insert
BEFORE INSERT ON statuses
WHEN NEW.quote_state NOT IN ('accepted', 'pending', 'rejected', 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'statuses.quote_state must be accepted, pending, rejected, or revoked');
END;

CREATE TRIGGER trg_statuses_quote_state_update
BEFORE UPDATE OF quote_state ON statuses
WHEN NEW.quote_state NOT IN ('accepted', 'pending', 'rejected', 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'statuses.quote_state must be accepted, pending, rejected, or revoked');
END;

CREATE TRIGGER trg_statuses_delete_polls
AFTER DELETE ON statuses
BEGIN
    DELETE FROM status_polls
    WHERE status_id = OLD.id;
END;

CREATE TRIGGER trg_statuses_null_local_reply_refs_after_delete
AFTER DELETE ON statuses
BEGIN
    UPDATE statuses
    SET in_reply_to_id = NULL
    WHERE in_reply_to_id = OLD.id;
END;

CREATE TRIGGER trg_status_polls_status_insert
BEFORE INSERT ON status_polls
WHEN NOT EXISTS (SELECT 1 FROM statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'status_polls.status_id must reference statuses.id');
END;

CREATE TRIGGER trg_status_polls_status_update
BEFORE UPDATE OF status_id ON status_polls
WHEN NOT EXISTS (SELECT 1 FROM statuses WHERE id = NEW.status_id)
BEGIN
    SELECT RAISE(ABORT, 'status_polls.status_id must reference statuses.id');
END;

CREATE TABLE scheduled_statuses_new (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    text_content TEXT NOT NULL DEFAULT '',
    visibility TEXT NOT NULL,
    spoiler_text TEXT NOT NULL DEFAULT '',
    sensitive INTEGER NOT NULL DEFAULT 0,
    language TEXT,
    quote_approval_policy TEXT,
    in_reply_to_id TEXT,
    media_ids_json TEXT NOT NULL DEFAULT '[]',
    poll_json TEXT,
    idempotency_key TEXT,
    application_id INTEGER,
    quote_of_uri TEXT,
    scheduled_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    claimed_at TEXT,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

INSERT INTO scheduled_statuses_new (
    id,
    account_id,
    text_content,
    visibility,
    spoiler_text,
    sensitive,
    language,
    quote_approval_policy,
    in_reply_to_id,
    media_ids_json,
    poll_json,
    idempotency_key,
    application_id,
    quote_of_uri,
    scheduled_at,
    created_at,
    updated_at,
    claimed_at
)
SELECT
    id,
    account_id,
    text_content,
    visibility,
    spoiler_text,
    sensitive,
    language,
    quote_approval_policy,
    in_reply_to_id,
    media_ids_json,
    poll_json,
    idempotency_key,
    application_id,
    quote_of_uri,
    scheduled_at,
    created_at,
    updated_at,
    claimed_at
FROM scheduled_statuses;

DROP TABLE scheduled_statuses;
ALTER TABLE scheduled_statuses_new RENAME TO scheduled_statuses;

CREATE INDEX IF NOT EXISTS idx_scheduled_statuses_account_schedule
    ON scheduled_statuses (account_id, scheduled_at, id);

CREATE INDEX IF NOT EXISTS idx_scheduled_statuses_due
    ON scheduled_statuses (scheduled_at, id);

PRAGMA foreign_keys = ON;
