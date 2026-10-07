#!/usr/bin/env node
/**
 * Backfill remote_status_mentions for remote statuses stored before migration
 * 112, from the Mention tags in their raw ActivityPub object. Mention
 * notifications read this table, so older mentions only notify after it runs.
 *
 * Rows match what crates/cfwdon-worker/src/statuses/status_mentions.rs writes
 * for hrefs it can resolve: a local account by its actor URL, or a stored
 * remote actor by its URI. Unresolvable hrefs are skipped.
 *
 *   node scripts/backfill_remote_status_mentions.mjs --instance-domain example.com --dry-run
 *   node scripts/backfill_remote_status_mentions.mjs --instance-domain example.com --remote
 */
import { spawnSync } from "node:child_process";

const args = new Set(process.argv.slice(2));
const database = valueAfter("--database") ?? "DB";
const remoteMode = args.has("--local") ? "--local" : "--remote";
const dryRun = args.has("--dry-run");
const instanceDomain = valueAfter("--instance-domain");

function valueAfter(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function sqlString(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

function wrangler(command) {
  const result = spawnSync(
    "wrangler",
    ["d1", "execute", database, remoteMode, "--json", "--command", command],
    { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
  );
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || `wrangler exited with ${result.status}`);
  }
  return JSON.parse(result.stdout);
}

function resultRows(output) {
  const first = Array.isArray(output) ? output[0] : output;
  return first?.results ?? first?.result?.[0]?.results ?? [];
}

export function backfillSql(domain) {
  const base = `https://${domain.replace(/^https?:\/\//, "").replace(/\/+$/, "")}/users/`;
  const localPrefix = sqlString(base);
  return `INSERT OR IGNORE INTO remote_status_mentions
  (status_id, mention_key, account_id, actor_uri, username, acct, url, published_at)
SELECT rs.id,
       lower(CASE WHEN a.id IS NOT NULL THEN a.username ELSE ra.username || '@' || ra.domain END),
       a.id,
       CASE WHEN a.id IS NULL THEN ra.actor_uri END,
       CASE WHEN a.id IS NOT NULL THEN a.username ELSE ra.username END,
       CASE WHEN a.id IS NOT NULL THEN a.username ELSE ra.username || '@' || ra.domain END,
       CASE WHEN a.id IS NOT NULL THEN ${localPrefix} || a.username
            ELSE COALESCE(ra.profile_url, ra.actor_uri) END,
       rs.published_at
FROM remote_statuses rs
JOIN json_each(rs.raw_object_json, '$.tag') t
LEFT JOIN remote_actors ra
  ON ra.actor_uri = json_extract(t.value, '$.href')
LEFT JOIN accounts a
  ON ra.actor_uri IS NULL
 AND lower(json_extract(t.value, '$.href')) = lower(${localPrefix} || a.username)
WHERE json_valid(rs.raw_object_json)
  AND json_type(rs.raw_object_json, '$.tag') = 'array'
  AND lower(json_extract(t.value, '$.type')) = 'mention'
  AND (a.id IS NOT NULL OR ra.actor_uri IS NOT NULL)
  AND NOT EXISTS (
      SELECT 1 FROM remote_status_mentions m WHERE m.status_id = rs.id
  )`;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  if (!instanceDomain) {
    console.error("--instance-domain is required (the INSTANCE_DOMAIN the worker runs with)");
    process.exit(2);
  }
  const sql = backfillSql(instanceDomain);
  if (dryRun) {
    console.log(sql);
    const pending = resultRows(
      wrangler(
        `SELECT COUNT(*) AS statuses
         FROM remote_statuses rs
         WHERE json_valid(rs.raw_object_json)
           AND json_type(rs.raw_object_json, '$.tag') = 'array'
           AND NOT EXISTS (SELECT 1 FROM remote_status_mentions m WHERE m.status_id = rs.id)`,
      ),
    );
    console.log(`statuses with tags and no mention rows: ${pending[0]?.statuses ?? 0}`);
  } else {
    const output = wrangler(sql);
    const first = Array.isArray(output) ? output[0] : output;
    console.log(`inserted ${first?.meta?.changes ?? 0} remote_status_mentions row(s)`);
  }
}
