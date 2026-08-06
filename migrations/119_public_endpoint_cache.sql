CREATE TABLE IF NOT EXISTS public_endpoint_cache (
    id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    computed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
