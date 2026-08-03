ALTER TABLE statuses
    ADD COLUMN card_json TEXT;

ALTER TABLE remote_statuses
    ADD COLUMN card_json TEXT;
