-- The home timeline, public timeline and tag timeline order remote statuses by
-- `published_at` compared as a string. ActivityPub `published` values arrive in
-- whatever RFC3339 shape the origin uses ("Z", "+09:00", with or without a
-- fractional part), and lexicographic order only matches chronological order
-- when every row shares one UTC, millisecond shape. New rows are normalized at
-- ingest; rewrite the existing ones here.
--
-- `strftime('%Y-%m-%dT%H:%M:%fZ', ...)` parses RFC3339 with an offset, converts
-- it to UTC and emits exactly three fractional digits. Rows whose timestamp
-- cannot be parsed are left untouched rather than nulled (the column is NOT
-- NULL).
UPDATE remote_statuses
   SET published_at = strftime('%Y-%m-%dT%H:%M:%fZ', published_at)
 WHERE published_at <> ''
   AND strftime('%Y-%m-%dT%H:%M:%fZ', published_at) IS NOT NULL
   AND strftime('%Y-%m-%dT%H:%M:%fZ', published_at) <> published_at;

-- Rows stored before published_at had a fallback have an empty timestamp and
-- would sort as the oldest entry and break cursor resolution. Use the receive
-- time, which is what the reblog path already falls back to.
UPDATE remote_statuses
   SET published_at = strftime('%Y-%m-%dT%H:%M:%fZ', created_at)
 WHERE published_at = ''
   AND strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NOT NULL;

-- `remote_status_hashtags` (trending windows) and `remote_status_mentions`
-- (mention ordering) copy `published_at` into their own NOT NULL columns and
-- compare it lexicographically too, so they need the same rewrite.
UPDATE remote_status_hashtags
   SET published_at = strftime('%Y-%m-%dT%H:%M:%fZ', published_at)
 WHERE published_at <> ''
   AND strftime('%Y-%m-%dT%H:%M:%fZ', published_at) IS NOT NULL
   AND strftime('%Y-%m-%dT%H:%M:%fZ', published_at) <> published_at;

UPDATE remote_status_mentions
   SET published_at = strftime('%Y-%m-%dT%H:%M:%fZ', published_at)
 WHERE published_at <> ''
   AND strftime('%Y-%m-%dT%H:%M:%fZ', published_at) IS NOT NULL
   AND strftime('%Y-%m-%dT%H:%M:%fZ', published_at) <> published_at;

-- Rows copied while the parent had no usable timestamp take the parent's
-- value, which the updates above already filled from its receive time.
UPDATE remote_status_hashtags
   SET published_at = (SELECT rs.published_at FROM remote_statuses rs WHERE rs.id = status_id)
 WHERE published_at = ''
   AND EXISTS (SELECT 1 FROM remote_statuses rs WHERE rs.id = status_id);

UPDATE remote_status_mentions
   SET published_at = (SELECT rs.published_at FROM remote_statuses rs WHERE rs.id = status_id)
 WHERE published_at = ''
   AND EXISTS (SELECT 1 FROM remote_statuses rs WHERE rs.id = status_id);
