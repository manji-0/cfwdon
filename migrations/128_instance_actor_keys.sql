-- The instance actor (`/actor`, Mastodon's representative `Application`)
-- signs the ActivityPub GETs this server makes on its own behalf, so a remote
-- in secure mode answers them without learning which local user asked. Its key
-- pair is generated on first use and kept in this single-row table.
-- `private_key_jwk` is encrypted with ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY when
-- that secret is configured, as account keys are.
CREATE TABLE IF NOT EXISTS instance_actor_keys (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    public_key_pem TEXT NOT NULL,
    private_key_jwk TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
