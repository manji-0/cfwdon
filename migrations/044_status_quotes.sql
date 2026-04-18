ALTER TABLE statuses
    ADD COLUMN quote_of_uri TEXT;

CREATE INDEX IF NOT EXISTS idx_statuses_quote_of_uri
    ON statuses (quote_of_uri);

ALTER TABLE remote_statuses
    ADD COLUMN quote_of_uri TEXT;

CREATE INDEX IF NOT EXISTS idx_remote_statuses_quote_of_uri
    ON remote_statuses (quote_of_uri);
