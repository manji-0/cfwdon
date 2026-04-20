ALTER TABLE accounts
ADD COLUMN default_quote_policy TEXT NOT NULL DEFAULT 'public';
