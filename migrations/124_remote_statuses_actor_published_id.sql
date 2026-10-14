-- The home timeline takes each followed actor's newest remote statuses in
-- (published_at, id) order. Without id in the index, that ORDER BY sorts the
-- tie in a temp B-tree and cannot walk the index alone.
DROP INDEX IF EXISTS idx_remote_statuses_actor_published;
CREATE INDEX idx_remote_statuses_actor_published
    ON remote_statuses (actor_uri, published_at DESC, id DESC);
