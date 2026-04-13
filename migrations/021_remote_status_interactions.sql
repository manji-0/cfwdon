CREATE TABLE IF NOT EXISTS remote_favourites (
    remote_actor_uri TEXT NOT NULL,
    status_id TEXT NOT NULL,
    target_uri TEXT NOT NULL,
    activity_uri TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (remote_actor_uri, target_uri),
    FOREIGN KEY (remote_actor_uri) REFERENCES remote_actors(actor_uri) ON DELETE CASCADE,
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_remote_favourites_status_id
    ON remote_favourites (status_id);

CREATE INDEX IF NOT EXISTS idx_remote_favourites_activity_uri
    ON remote_favourites (activity_uri);

CREATE TABLE IF NOT EXISTS remote_reblogs (
    remote_actor_uri TEXT NOT NULL,
    status_id TEXT NOT NULL,
    target_uri TEXT NOT NULL,
    activity_uri TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (remote_actor_uri, target_uri),
    FOREIGN KEY (remote_actor_uri) REFERENCES remote_actors(actor_uri) ON DELETE CASCADE,
    FOREIGN KEY (status_id) REFERENCES statuses(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_remote_reblogs_status_id
    ON remote_reblogs (status_id);

CREATE INDEX IF NOT EXISTS idx_remote_reblogs_activity_uri
    ON remote_reblogs (activity_uri);
