CREATE TABLE IF NOT EXISTS async_refreshes (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    result_count INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_async_refreshes_updated_at
    ON async_refreshes (updated_at DESC);
