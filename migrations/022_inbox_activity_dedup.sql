CREATE TABLE IF NOT EXISTS inbox_activities (
    actor_uri TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    activity_type TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    processed_at TEXT,
    PRIMARY KEY (actor_uri, activity_id)
);

CREATE INDEX IF NOT EXISTS idx_inbox_activities_processed
    ON inbox_activities (processed_at, created_at);
