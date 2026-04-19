ALTER TABLE accounts ADD COLUMN locked INTEGER NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN bot INTEGER NOT NULL DEFAULT 0;

UPDATE accounts
SET bot = COALESCE(
    (
        SELECT aps.bot
        FROM account_profile_settings aps
        WHERE aps.account_id = accounts.id
    ),
    0
);
