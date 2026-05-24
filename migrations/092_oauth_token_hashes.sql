ALTER TABLE oauth_access_tokens
ADD COLUMN access_token_hash TEXT;

CREATE INDEX IF NOT EXISTS idx_oauth_access_tokens_access_token_hash
    ON oauth_access_tokens (access_token_hash);

ALTER TABLE oauth_app_access_tokens
ADD COLUMN access_token_hash TEXT;

CREATE INDEX IF NOT EXISTS idx_oauth_app_access_tokens_access_token_hash
    ON oauth_app_access_tokens (access_token_hash);
