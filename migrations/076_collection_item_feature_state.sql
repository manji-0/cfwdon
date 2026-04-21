ALTER TABLE account_collection_items
    ADD COLUMN activity_uri TEXT;

ALTER TABLE account_collection_items
    ADD COLUMN feature_authorization TEXT;

CREATE INDEX IF NOT EXISTS idx_account_collection_items_activity_uri
    ON account_collection_items (activity_uri)
    WHERE activity_uri IS NOT NULL;
