-- Mastodon `IpBlock`: `ip` is a canonical CIDR range (`203.0.113.0/24`).
-- `expires_at` is RFC3339 UTC, NULL for a permanent block.
CREATE TABLE IF NOT EXISTS ip_blocks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ip TEXT NOT NULL UNIQUE,
    severity TEXT NOT NULL
        CHECK (severity IN ('sign_up_requires_approval', 'sign_up_block', 'no_access')),
    comment TEXT NOT NULL DEFAULT '',
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
