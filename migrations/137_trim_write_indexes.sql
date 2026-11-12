-- D1 bills a written row for every index entry an INSERT or DELETE touches.
-- Remote statuses arrive with each federated Create/Announce and the retention
-- purge deletes them again, so every index entry here is paid twice. Index
-- only the rows a lookup can find, and drop indexes no query needs.
--
-- A partial index `WHERE col IS NOT NULL` serves every lookup that filters
-- `col = ?`, `col IN (...)` or joins on `col = other`. Every query that used
-- the indexes below does; scripts/check_query_plans.py pins those plans.

-- remote_statuses: most rows are neither boosts, quotes nor replies.
-- `(quote_of_uri, quote_state)` already serves `quote_of_uri = ?` lookups.
-- No query seeks by `in_reply_to_id`: readers join `parent.id = rs.in_reply_to_id`.
DROP INDEX IF EXISTS idx_remote_statuses_quote_of_uri;
DROP INDEX IF EXISTS idx_remote_statuses_quote_of_uri_state;
CREATE INDEX idx_remote_statuses_quote_of_uri_state
    ON remote_statuses (quote_of_uri, quote_state)
    WHERE quote_of_uri IS NOT NULL;
DROP INDEX IF EXISTS idx_remote_statuses_boost_of_uri;
CREATE INDEX idx_remote_statuses_boost_of_uri
    ON remote_statuses (boost_of_uri)
    WHERE boost_of_uri IS NOT NULL;
DROP INDEX IF EXISTS idx_remote_statuses_url;
CREATE INDEX idx_remote_statuses_url
    ON remote_statuses (url)
    WHERE url IS NOT NULL;
DROP INDEX IF EXISTS idx_remote_statuses_in_reply_to_uri;
CREATE INDEX idx_remote_statuses_in_reply_to_uri
    ON remote_statuses (in_reply_to_uri)
    WHERE in_reply_to_uri IS NOT NULL;
DROP INDEX IF EXISTS idx_remote_statuses_in_reply_to_id;

-- statuses: the same shape for local posts.
DROP INDEX IF EXISTS idx_statuses_quote_of_uri;
DROP INDEX IF EXISTS idx_statuses_quote_of_uri_state;
CREATE INDEX idx_statuses_quote_of_uri_state
    ON statuses (quote_of_uri, quote_state)
    WHERE quote_of_uri IS NOT NULL;
DROP INDEX IF EXISTS idx_statuses_boost_of_uri;
CREATE INDEX idx_statuses_boost_of_uri
    ON statuses (boost_of_uri)
    WHERE boost_of_uri IS NOT NULL;
DROP INDEX IF EXISTS idx_statuses_in_reply_to_id;
CREATE INDEX idx_statuses_in_reply_to_id
    ON statuses (in_reply_to_id)
    WHERE in_reply_to_id IS NOT NULL;

-- Favourites and boosts: the partial `(status_id, created_at)` and
-- `(remote_status_id, created_at)` indexes serve every lookup by target,
-- foreign-key cascades included. No query filters on `ap_activity_id`, and
-- nothing orders a viewer's favourites by `created_at` (saved pages walk
-- `idx_favourites_account_id` in rowid order).
DROP INDEX IF EXISTS idx_favourites_status_id;
DROP INDEX IF EXISTS idx_favourites_remote_status_id;
DROP INDEX IF EXISTS idx_favourites_ap_activity_id;
DROP INDEX IF EXISTS idx_favourites_account_created_at;
DROP INDEX IF EXISTS idx_reblogs_status_id;
DROP INDEX IF EXISTS idx_reblogs_remote_status_id;
DROP INDEX IF EXISTS idx_reblogs_ap_activity_id;
DROP INDEX IF EXISTS idx_remote_favourites_status_id;
DROP INDEX IF EXISTS idx_remote_reblogs_status_id;

-- Bookmarks: each row targets either a local or a remote status, and saved
-- pages walk `idx_bookmarks_account_id`.
DROP INDEX IF EXISTS idx_bookmarks_account_created_at;
DROP INDEX IF EXISTS idx_bookmarks_status_id;
CREATE INDEX idx_bookmarks_status_id
    ON bookmarks (status_id)
    WHERE status_id IS NOT NULL;
DROP INDEX IF EXISTS idx_bookmarks_remote_status_id;
CREATE INDEX idx_bookmarks_remote_status_id
    ON bookmarks (remote_status_id)
    WHERE remote_status_id IS NOT NULL;

-- `status_id` is UNIQUE, so its automatic index already serves lookups.
DROP INDEX IF EXISTS idx_remote_status_polls_status_id;

-- Delivery fan-out reads `(account_id, shared_inbox_uri, inbox_uri)`; no
-- query filters on `inbox_uri` alone.
DROP INDEX IF EXISTS idx_followers_account_inbox;

-- Follows of remote actors have no local target account.
DROP INDEX IF EXISTS idx_follows_target_state;
CREATE INDEX idx_follows_target_state
    ON follows (target_account_id, state, created_at DESC)
    WHERE target_account_id IS NOT NULL;
