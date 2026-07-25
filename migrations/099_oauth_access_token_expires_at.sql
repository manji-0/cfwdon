ALTER TABLE oauth_access_tokens
ADD COLUMN expires_at INTEGER;

CREATE INDEX IF NOT EXISTS idx_oauth_access_tokens_expires_at
    ON oauth_access_tokens (expires_at);
