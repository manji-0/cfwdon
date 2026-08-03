CREATE TABLE IF NOT EXISTS trending_tags_cache (
    id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    computed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
