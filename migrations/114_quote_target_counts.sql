CREATE TABLE IF NOT EXISTS quote_target_counts (
    target_uri TEXT PRIMARY KEY,
    quotes_count INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT OR REPLACE INTO quote_target_counts (target_uri, quotes_count, updated_at)
SELECT target_uri, COUNT(*) AS quotes_count, CURRENT_TIMESTAMP
FROM (
    SELECT quote_of_uri AS target_uri
    FROM statuses
    WHERE quote_of_uri IS NOT NULL
      AND quote_state = 'accepted'
    UNION ALL
    SELECT quote_of_uri AS target_uri
    FROM remote_statuses
    WHERE quote_of_uri IS NOT NULL
      AND quote_state = 'accepted'
)
GROUP BY target_uri;

CREATE TRIGGER IF NOT EXISTS trg_statuses_quote_counts_insert
AFTER INSERT ON statuses
WHEN NEW.quote_of_uri IS NOT NULL AND NEW.quote_state = 'accepted'
BEGIN
    INSERT INTO quote_target_counts (target_uri, quotes_count, updated_at)
    VALUES (NEW.quote_of_uri, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(target_uri) DO UPDATE SET
        quotes_count = quotes_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_quote_counts_delete
AFTER DELETE ON statuses
WHEN OLD.quote_of_uri IS NOT NULL AND OLD.quote_state = 'accepted'
BEGIN
    UPDATE quote_target_counts
    SET quotes_count = max(quotes_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE target_uri = OLD.quote_of_uri;
END;

CREATE TRIGGER IF NOT EXISTS trg_statuses_quote_counts_update
AFTER UPDATE OF quote_of_uri, quote_state ON statuses
BEGIN
    UPDATE quote_target_counts
    SET quotes_count = max(quotes_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE OLD.quote_of_uri IS NOT NULL
      AND OLD.quote_state = 'accepted'
      AND (
            NEW.quote_of_uri IS NULL
            OR NEW.quote_of_uri <> OLD.quote_of_uri
            OR NEW.quote_state <> 'accepted'
          )
      AND target_uri = OLD.quote_of_uri;

    INSERT INTO quote_target_counts (target_uri, quotes_count, updated_at)
    SELECT NEW.quote_of_uri, 1, CURRENT_TIMESTAMP
    WHERE NEW.quote_of_uri IS NOT NULL
      AND NEW.quote_state = 'accepted'
      AND (
            OLD.quote_of_uri IS NULL
            OR OLD.quote_of_uri <> NEW.quote_of_uri
            OR OLD.quote_state <> 'accepted'
          )
    ON CONFLICT(target_uri) DO UPDATE SET
        quotes_count = quotes_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_quote_counts_insert
AFTER INSERT ON remote_statuses
WHEN NEW.quote_of_uri IS NOT NULL AND NEW.quote_state = 'accepted'
BEGIN
    INSERT INTO quote_target_counts (target_uri, quotes_count, updated_at)
    VALUES (NEW.quote_of_uri, 1, CURRENT_TIMESTAMP)
    ON CONFLICT(target_uri) DO UPDATE SET
        quotes_count = quotes_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_quote_counts_delete
AFTER DELETE ON remote_statuses
WHEN OLD.quote_of_uri IS NOT NULL AND OLD.quote_state = 'accepted'
BEGIN
    UPDATE quote_target_counts
    SET quotes_count = max(quotes_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE target_uri = OLD.quote_of_uri;
END;

CREATE TRIGGER IF NOT EXISTS trg_remote_statuses_quote_counts_update
AFTER UPDATE OF quote_of_uri, quote_state ON remote_statuses
BEGIN
    UPDATE quote_target_counts
    SET quotes_count = max(quotes_count - 1, 0),
        updated_at = CURRENT_TIMESTAMP
    WHERE OLD.quote_of_uri IS NOT NULL
      AND OLD.quote_state = 'accepted'
      AND (
            NEW.quote_of_uri IS NULL
            OR NEW.quote_of_uri <> OLD.quote_of_uri
            OR NEW.quote_state <> 'accepted'
          )
      AND target_uri = OLD.quote_of_uri;

    INSERT INTO quote_target_counts (target_uri, quotes_count, updated_at)
    SELECT NEW.quote_of_uri, 1, CURRENT_TIMESTAMP
    WHERE NEW.quote_of_uri IS NOT NULL
      AND NEW.quote_state = 'accepted'
      AND (
            OLD.quote_of_uri IS NULL
            OR OLD.quote_of_uri <> NEW.quote_of_uri
            OR OLD.quote_state <> 'accepted'
          )
    ON CONFLICT(target_uri) DO UPDATE SET
        quotes_count = quotes_count + 1,
        updated_at = CURRENT_TIMESTAMP;
END;
