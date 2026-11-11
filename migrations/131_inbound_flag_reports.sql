-- Reports filed by other servers through an ActivityPub `Flag`.
-- `uri` is the `Flag` id, set only for remote reports; one `Flag` files at most
-- one report per target account, so redelivery files nothing new.
-- `origin_domain` is the reporting server's domain; NULL marks a local report.
ALTER TABLE reports ADD COLUMN uri TEXT;
ALTER TABLE reports ADD COLUMN origin_domain TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_reports_uri_target_account
    ON reports (uri, target_account_id)
    WHERE uri IS NOT NULL;
