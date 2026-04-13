ALTER TABLE accounts ADD COLUMN bio_text TEXT NOT NULL DEFAULT '';
ALTER TABLE accounts ADD COLUMN default_post_visibility TEXT NOT NULL DEFAULT 'public';
ALTER TABLE accounts ADD COLUMN default_sensitive INTEGER NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN default_language TEXT;
