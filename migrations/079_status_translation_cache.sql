CREATE TABLE IF NOT EXISTS status_translation_cache (
    status_id TEXT NOT NULL,
    target_language TEXT NOT NULL,
    provider TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    translation_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (status_id, target_language, provider)
);
