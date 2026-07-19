-- Keep a single remote_actors row per acct handle (username@domain).
DELETE FROM remote_actors
WHERE rowid NOT IN (
    SELECT MAX(rowid)
    FROM remote_actors
    GROUP BY lower(username), lower(domain)
);

DROP INDEX IF EXISTS idx_remote_actors_domain_username;

CREATE UNIQUE INDEX IF NOT EXISTS idx_remote_actors_domain_username_unique
    ON remote_actors (lower(username), lower(domain));
