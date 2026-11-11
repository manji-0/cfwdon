-- Mastodon `DomainBlock` fields. Existing rows were enforced as suspensions,
-- so they keep that severity.
ALTER TABLE instance_domain_blocks
    ADD COLUMN severity TEXT NOT NULL DEFAULT 'suspend'
    CHECK (severity IN ('suspend', 'silence', 'noop'));
ALTER TABLE instance_domain_blocks ADD COLUMN reject_media INTEGER NOT NULL DEFAULT 0;
ALTER TABLE instance_domain_blocks ADD COLUMN reject_reports INTEGER NOT NULL DEFAULT 0;
ALTER TABLE instance_domain_blocks ADD COLUMN obfuscate INTEGER NOT NULL DEFAULT 0;
ALTER TABLE instance_domain_blocks ADD COLUMN private_comment TEXT;
ALTER TABLE instance_domain_blocks ADD COLUMN public_comment TEXT;
