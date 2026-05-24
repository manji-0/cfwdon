CREATE TABLE IF NOT EXISTS account_private_keys (
    account_id TEXT PRIMARY KEY,
    private_key_jwk_encrypted TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
