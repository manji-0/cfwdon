ALTER TABLE pending_email_confirmations
    ADD COLUMN confirmation_token TEXT;

ALTER TABLE pending_email_confirmations
    ADD COLUMN confirmation_sent_at TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_pending_email_confirmations_token
    ON pending_email_confirmations (confirmation_token);
