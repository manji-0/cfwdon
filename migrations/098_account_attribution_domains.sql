ALTER TABLE account_profile_settings
ADD COLUMN attribution_domains_json TEXT NOT NULL DEFAULT '[]';
