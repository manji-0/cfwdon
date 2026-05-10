CREATE TABLE IF NOT EXISTS account_stats (
    account_id TEXT PRIMARY KEY,
    followers_count INTEGER NOT NULL DEFAULT 0,
    following_count INTEGER NOT NULL DEFAULT 0,
    statuses_count INTEGER NOT NULL DEFAULT 0,
    last_status_at TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

INSERT OR REPLACE INTO account_stats (
    account_id,
    followers_count,
    following_count,
    statuses_count,
    last_status_at,
    updated_at
)
SELECT
    a.id,
    (
        SELECT COUNT(*)
        FROM followers rf
        WHERE rf.account_id = a.id
    ) + (
        SELECT COUNT(*)
        FROM follows lf
        WHERE lf.target_account_id = a.id
          AND lf.state = 'accepted'
    ),
    (
        SELECT COUNT(*)
        FROM follows f
        WHERE f.follower_account_id = a.id
          AND f.state = 'accepted'
    ),
    (
        SELECT COUNT(*)
        FROM statuses s
        WHERE s.account_id = a.id
    ),
    (
        SELECT MAX(substr(s.created_at, 1, 10))
        FROM statuses s
        WHERE s.account_id = a.id
    ),
    CURRENT_TIMESTAMP
FROM accounts a;

CREATE TRIGGER IF NOT EXISTS trg_statuses_account_stats_insert
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

CREATE TRIGGER IF NOT EXISTS trg_statuses_account_stats_delete
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

CREATE TRIGGER IF NOT EXISTS trg_statuses_account_stats_update_old
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

CREATE TRIGGER IF NOT EXISTS trg_statuses_account_stats_update_new
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

CREATE TRIGGER IF NOT EXISTS trg_followers_account_stats_insert
AFTER INSERT ON followers
BEGIN
    INSERT INTO account_stats (account_id, followers_count, updated_at)
    VALUES (NEW.account_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(account_id) DO UPDATE SET
        followers_count = followers_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_followers_account_stats_delete
AFTER DELETE ON followers
BEGIN
    UPDATE account_stats
    SET followers_count = max(followers_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.account_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_followers_account_stats_update_old
AFTER UPDATE ON followers
WHEN NEW.account_id <> OLD.account_id
BEGIN
    UPDATE account_stats
    SET followers_count = max(followers_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.account_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_followers_account_stats_update_new
AFTER UPDATE ON followers
WHEN NEW.account_id <> OLD.account_id
BEGIN
    INSERT INTO account_stats (account_id, followers_count, updated_at)
    VALUES (NEW.account_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(account_id) DO UPDATE SET
        followers_count = followers_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_follows_account_stats_insert_following
AFTER INSERT ON follows
WHEN NEW.state = 'accepted'
BEGIN
    INSERT INTO account_stats (account_id, following_count, updated_at)
    VALUES (NEW.follower_account_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(account_id) DO UPDATE SET
        following_count = following_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_follows_account_stats_delete_following
AFTER DELETE ON follows
WHEN OLD.state = 'accepted'
BEGIN
    UPDATE account_stats
    SET following_count = max(following_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.follower_account_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_follows_account_stats_update_following_old
AFTER UPDATE ON follows
WHEN OLD.state = 'accepted'
 AND (NEW.state <> 'accepted' OR NEW.follower_account_id <> OLD.follower_account_id)
BEGIN
    UPDATE account_stats
    SET following_count = max(following_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.follower_account_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_follows_account_stats_update_following_new
AFTER UPDATE ON follows
WHEN NEW.state = 'accepted'
 AND (OLD.state <> 'accepted' OR NEW.follower_account_id <> OLD.follower_account_id)
BEGIN
    INSERT INTO account_stats (account_id, following_count, updated_at)
    VALUES (NEW.follower_account_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(account_id) DO UPDATE SET
        following_count = following_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_follows_account_stats_insert_follower
AFTER INSERT ON follows
WHEN NEW.state = 'accepted'
 AND NEW.target_account_id IS NOT NULL
BEGIN
    INSERT INTO account_stats (account_id, followers_count, updated_at)
    VALUES (NEW.target_account_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(account_id) DO UPDATE SET
        followers_count = followers_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_follows_account_stats_delete_follower
AFTER DELETE ON follows
WHEN OLD.state = 'accepted'
 AND OLD.target_account_id IS NOT NULL
BEGIN
    UPDATE account_stats
    SET followers_count = max(followers_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.target_account_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_follows_account_stats_update_follower_old
AFTER UPDATE ON follows
WHEN OLD.state = 'accepted'
 AND OLD.target_account_id IS NOT NULL
 AND (
    NEW.state <> 'accepted'
    OR NEW.target_account_id IS NULL
    OR NEW.target_account_id <> OLD.target_account_id
 )
BEGIN
    UPDATE account_stats
    SET followers_count = max(followers_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE account_id = OLD.target_account_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_follows_account_stats_update_follower_new
AFTER UPDATE ON follows
WHEN NEW.state = 'accepted'
 AND NEW.target_account_id IS NOT NULL
 AND (
    OLD.state <> 'accepted'
    OR OLD.target_account_id IS NULL
    OR NEW.target_account_id <> OLD.target_account_id
 )
BEGIN
    INSERT INTO account_stats (account_id, followers_count, updated_at)
    VALUES (NEW.target_account_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(account_id) DO UPDATE SET
        followers_count = followers_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;
