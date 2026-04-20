ALTER TABLE remote_statuses
    ADD COLUMN quote_state TEXT NOT NULL DEFAULT 'accepted';

CREATE INDEX IF NOT EXISTS idx_remote_statuses_quote_of_uri_state
    ON remote_statuses (quote_of_uri, quote_state);
