ALTER TABLE status_polls
    ADD COLUMN federated_closed_at TEXT;

CREATE INDEX idx_status_polls_federated_closed_at
    ON status_polls (federated_closed_at, expires_at);
