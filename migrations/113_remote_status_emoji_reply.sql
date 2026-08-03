ALTER TABLE remote_statuses
    ADD COLUMN federated_emojis_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE remote_statuses
    ADD COLUMN in_reply_to_id TEXT;
