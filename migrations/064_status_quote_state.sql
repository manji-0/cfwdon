ALTER TABLE statuses
    ADD COLUMN quote_state TEXT NOT NULL DEFAULT 'accepted';

CREATE INDEX IF NOT EXISTS idx_statuses_quote_of_uri_state
    ON statuses (quote_of_uri, quote_state);
