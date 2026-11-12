-- NOISE=<n> for run.sh: n statuses by an account nobody in the scenario
-- follows (qcf6), each favourited by qcf5, and n remote statuses by an
-- unfollowed actor, all older than the scenario. A request whose rows read
-- grows with n reads rows it does not show.
WITH RECURSIVE series(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM series WHERE n < @N@)
INSERT INTO statuses (id, account_id, content_html, text_content, visibility, created_at, updated_at)
SELECT printf('noise%07d', n), '100000000000000008', '<p>noise</p>', 'noise', 'public',
       strftime('%Y-%m-%dT%H:%M:%SZ', '2020-01-01', '+' || n || ' seconds'),
       strftime('%Y-%m-%dT%H:%M:%SZ', '2020-01-01', '+' || n || ' seconds')
FROM series;
WITH RECURSIVE series(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM series WHERE n < @N@)
INSERT INTO favourites (account_id, status_id, target_uri, created_at, updated_at)
SELECT '100000000000000007', printf('noise%07d', n), 'local:' || printf('noise%07d', n),
       '2020-01-02T00:00:00Z', '2020-01-02T00:00:00Z'
FROM series;
INSERT INTO remote_actors (actor_uri, username, domain, inbox_uri, public_key_id, public_key_pem)
VALUES ('https://noise.example/users/n', 'n', 'noise.example', 'https://noise.example/inbox',
        'https://noise.example/users/n#main-key', 'x');
WITH RECURSIVE series(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM series WHERE n < @N@)
INSERT INTO remote_statuses (id, actor_uri, object_uri, content_html, visibility, published_at, raw_object_json)
SELECT printf('rnoise%07d', n), 'https://noise.example/users/n', printf('https://noise.example/statuses/%d', n),
       '<p>noise</p>', 'public', '2020-01-01T00:00:00Z', '{}'
FROM series;
