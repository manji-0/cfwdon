-- Favourites and bookmarks page by rowid (Mastodon pages them by record id).
-- A single-column index on account_id keeps entries in rowid order, so these
-- pages need no sort.
CREATE INDEX IF NOT EXISTS idx_favourites_account_id
    ON favourites (account_id);

CREATE INDEX IF NOT EXISTS idx_bookmarks_account_id
    ON bookmarks (account_id);
