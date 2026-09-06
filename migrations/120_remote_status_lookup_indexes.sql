-- Point lookups on url / in_reply_to_uri currently table-scan because SQLite
-- cannot use the object_uri UNIQUE index when the predicate is OR-combined,
-- and those columns had no indexes of their own. The purge job also probes
-- followers by actor_uri, which was only indexed as (account_id, actor_uri).
CREATE INDEX IF NOT EXISTS idx_remote_statuses_url
    ON remote_statuses (url);

CREATE INDEX IF NOT EXISTS idx_remote_statuses_in_reply_to_uri
    ON remote_statuses (in_reply_to_uri);

CREATE INDEX IF NOT EXISTS idx_followers_actor_uri
    ON followers (actor_uri);
