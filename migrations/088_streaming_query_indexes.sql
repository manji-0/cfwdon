CREATE INDEX IF NOT EXISTS idx_statuses_created_id
    ON statuses (created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_remote_statuses_published_id
    ON remote_statuses (published_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_favourites_created_status_account
    ON favourites (created_at DESC, status_id, account_id)
    WHERE status_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_remote_favourites_created_status_actor
    ON remote_favourites (created_at DESC, status_id, remote_actor_uri);

CREATE INDEX IF NOT EXISTS idx_reblogs_created_status_account
    ON reblogs (created_at DESC, status_id, account_id)
    WHERE status_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_remote_reblogs_created_status_actor
    ON remote_reblogs (created_at DESC, status_id, remote_actor_uri);
