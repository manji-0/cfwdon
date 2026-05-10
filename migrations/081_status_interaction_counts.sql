CREATE TABLE IF NOT EXISTS status_counts (
    status_id TEXT PRIMARY KEY,
    favourites_count INTEGER NOT NULL DEFAULT 0,
    reblogs_count INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS remote_status_counts (
    remote_status_id TEXT PRIMARY KEY,
    favourites_count INTEGER NOT NULL DEFAULT 0,
    reblogs_count INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (remote_status_id) REFERENCES remote_statuses(id) ON DELETE CASCADE
);

INSERT OR REPLACE INTO status_counts (status_id, favourites_count, reblogs_count, updated_at)
SELECT
    s.id,
    (
        SELECT COUNT(*)
        FROM favourites f
        WHERE f.status_id = s.id
    ) + (
        SELECT COUNT(*)
        FROM remote_favourites rf
        WHERE rf.status_id = s.id
    ),
    (
        SELECT COUNT(*)
        FROM reblogs r
        WHERE r.status_id = s.id
    ) + (
        SELECT COUNT(*)
        FROM remote_reblogs rr
        WHERE rr.status_id = s.id
    ),
    CURRENT_TIMESTAMP
FROM statuses s;

INSERT OR REPLACE INTO remote_status_counts (remote_status_id, favourites_count, reblogs_count, updated_at)
SELECT
    rs.id,
    (
        SELECT COUNT(*)
        FROM favourites f
        WHERE f.remote_status_id = rs.id
    ),
    (
        SELECT COUNT(*)
        FROM reblogs r
        WHERE r.remote_status_id = rs.id
    ),
    CURRENT_TIMESTAMP
FROM remote_statuses rs;

CREATE TRIGGER IF NOT EXISTS trg_favourites_status_counts_insert
AFTER INSERT ON favourites
WHEN NEW.status_id IS NOT NULL
BEGIN
    INSERT INTO status_counts (status_id, favourites_count, updated_at)
    VALUES (NEW.status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(status_id) DO UPDATE SET
        favourites_count = favourites_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_favourites_status_counts_update_old
AFTER UPDATE ON favourites
WHEN OLD.status_id IS NOT NULL
 AND (NEW.status_id IS NULL OR NEW.status_id <> OLD.status_id)
BEGIN
    UPDATE status_counts
    SET favourites_count = max(favourites_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_favourites_status_counts_update_new
AFTER UPDATE ON favourites
WHEN NEW.status_id IS NOT NULL
 AND (OLD.status_id IS NULL OR NEW.status_id <> OLD.status_id)
BEGIN
    INSERT INTO status_counts (status_id, favourites_count, updated_at)
    VALUES (NEW.status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(status_id) DO UPDATE SET
        favourites_count = favourites_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_favourites_status_counts_delete
AFTER DELETE ON favourites
WHEN OLD.status_id IS NOT NULL
BEGIN
    UPDATE status_counts
    SET favourites_count = max(favourites_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_favourites_remote_status_counts_insert
AFTER INSERT ON favourites
WHEN NEW.remote_status_id IS NOT NULL
BEGIN
    INSERT INTO remote_status_counts (remote_status_id, favourites_count, updated_at)
    VALUES (NEW.remote_status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(remote_status_id) DO UPDATE SET
        favourites_count = favourites_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_favourites_remote_status_counts_update_old
AFTER UPDATE ON favourites
WHEN OLD.remote_status_id IS NOT NULL
 AND (NEW.remote_status_id IS NULL OR NEW.remote_status_id <> OLD.remote_status_id)
BEGIN
    UPDATE remote_status_counts
    SET favourites_count = max(favourites_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE remote_status_id = OLD.remote_status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_favourites_remote_status_counts_update_new
AFTER UPDATE ON favourites
WHEN NEW.remote_status_id IS NOT NULL
 AND (OLD.remote_status_id IS NULL OR NEW.remote_status_id <> OLD.remote_status_id)
BEGIN
    INSERT INTO remote_status_counts (remote_status_id, favourites_count, updated_at)
    VALUES (NEW.remote_status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(remote_status_id) DO UPDATE SET
        favourites_count = favourites_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_favourites_remote_status_counts_delete
AFTER DELETE ON favourites
WHEN OLD.remote_status_id IS NOT NULL
BEGIN
    UPDATE remote_status_counts
    SET favourites_count = max(favourites_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE remote_status_id = OLD.remote_status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_favourites_status_counts_insert
AFTER INSERT ON remote_favourites
BEGIN
    INSERT INTO status_counts (status_id, favourites_count, updated_at)
    VALUES (NEW.status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(status_id) DO UPDATE SET
        favourites_count = favourites_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_favourites_status_counts_update_old
AFTER UPDATE ON remote_favourites
WHEN NEW.status_id <> OLD.status_id
BEGIN
    UPDATE status_counts
    SET favourites_count = max(favourites_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_favourites_status_counts_update_new
AFTER UPDATE ON remote_favourites
WHEN NEW.status_id <> OLD.status_id
BEGIN
    INSERT INTO status_counts (status_id, favourites_count, updated_at)
    VALUES (NEW.status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(status_id) DO UPDATE SET
        favourites_count = favourites_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_favourites_status_counts_delete
AFTER DELETE ON remote_favourites
BEGIN
    UPDATE status_counts
    SET favourites_count = max(favourites_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_status_counts_insert
AFTER INSERT ON reblogs
WHEN NEW.status_id IS NOT NULL
BEGIN
    INSERT INTO status_counts (status_id, reblogs_count, updated_at)
    VALUES (NEW.status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(status_id) DO UPDATE SET
        reblogs_count = reblogs_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_status_counts_update_old
AFTER UPDATE ON reblogs
WHEN OLD.status_id IS NOT NULL
 AND (NEW.status_id IS NULL OR NEW.status_id <> OLD.status_id)
BEGIN
    UPDATE status_counts
    SET reblogs_count = max(reblogs_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_status_counts_update_new
AFTER UPDATE ON reblogs
WHEN NEW.status_id IS NOT NULL
 AND (OLD.status_id IS NULL OR NEW.status_id <> OLD.status_id)
BEGIN
    INSERT INTO status_counts (status_id, reblogs_count, updated_at)
    VALUES (NEW.status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(status_id) DO UPDATE SET
        reblogs_count = reblogs_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_status_counts_delete
AFTER DELETE ON reblogs
WHEN OLD.status_id IS NOT NULL
BEGIN
    UPDATE status_counts
    SET reblogs_count = max(reblogs_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_remote_status_counts_insert
AFTER INSERT ON reblogs
WHEN NEW.remote_status_id IS NOT NULL
BEGIN
    INSERT INTO remote_status_counts (remote_status_id, reblogs_count, updated_at)
    VALUES (NEW.remote_status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(remote_status_id) DO UPDATE SET
        reblogs_count = reblogs_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_remote_status_counts_update_old
AFTER UPDATE ON reblogs
WHEN OLD.remote_status_id IS NOT NULL
 AND (NEW.remote_status_id IS NULL OR NEW.remote_status_id <> OLD.remote_status_id)
BEGIN
    UPDATE remote_status_counts
    SET reblogs_count = max(reblogs_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE remote_status_id = OLD.remote_status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_remote_status_counts_update_new
AFTER UPDATE ON reblogs
WHEN NEW.remote_status_id IS NOT NULL
 AND (OLD.remote_status_id IS NULL OR NEW.remote_status_id <> OLD.remote_status_id)
BEGIN
    INSERT INTO remote_status_counts (remote_status_id, reblogs_count, updated_at)
    VALUES (NEW.remote_status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(remote_status_id) DO UPDATE SET
        reblogs_count = reblogs_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_reblogs_remote_status_counts_delete
AFTER DELETE ON reblogs
WHEN OLD.remote_status_id IS NOT NULL
BEGIN
    UPDATE remote_status_counts
    SET reblogs_count = max(reblogs_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE remote_status_id = OLD.remote_status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_reblogs_status_counts_insert
AFTER INSERT ON remote_reblogs
BEGIN
    INSERT INTO status_counts (status_id, reblogs_count, updated_at)
    VALUES (NEW.status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(status_id) DO UPDATE SET
        reblogs_count = reblogs_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_reblogs_status_counts_update_old
AFTER UPDATE ON remote_reblogs
WHEN NEW.status_id <> OLD.status_id
BEGIN
    UPDATE status_counts
    SET reblogs_count = max(reblogs_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.status_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_reblogs_status_counts_update_new
AFTER UPDATE ON remote_reblogs
WHEN NEW.status_id <> OLD.status_id
BEGIN
    INSERT INTO status_counts (status_id, reblogs_count, updated_at)
    VALUES (NEW.status_id, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(status_id) DO UPDATE SET
        reblogs_count = reblogs_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_reblogs_status_counts_delete
AFTER DELETE ON remote_reblogs
BEGIN
    UPDATE status_counts
    SET reblogs_count = max(reblogs_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE status_id = OLD.status_id;
END;
