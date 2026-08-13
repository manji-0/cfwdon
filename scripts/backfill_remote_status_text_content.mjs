#!/usr/bin/env node
/**
 * Backfill remote_statuses.text_content for rows written before migration 108 /
 * commit 6456d69, matching domain strip_basic_html_tags(content_html).
 *
 *   node scripts/backfill_remote_status_text_content.mjs --dry-run
 *   node scripts/backfill_remote_status_text_content.mjs --remote
 */
import { spawnSync } from "node:child_process";

const args = new Set(process.argv.slice(2));
const database = valueAfter("--database") ?? "DB";
const remoteMode = args.has("--local") ? "--local" : "--remote";
const dryRun = args.has("--dry-run");
const batchSize = Number(valueAfter("--batch-size") ?? "40");

function valueAfter(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function sqlString(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

/** Mirrors crates/cfwdon-domain/src/remote/status.rs::strip_basic_html_tags */
function stripBasicHtmlTags(html) {
  let output = "";
  let inTag = false;
  for (const ch of html) {
    if (ch === "<") {
      inTag = true;
    } else if (ch === ">") {
      inTag = false;
    } else if (!inTag) {
      output += ch;
    }
  }
  return output;
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

function executeUpdates(statements) {
  if (statements.length === 0) {
    return;
  }
  const command = statements.join(";\n");
  if (dryRun) {
    console.log(command);
    return;
  }
  wrangler(command);
}

const rows = resultRows(
  wrangler(
    `SELECT id, content_html
     FROM remote_statuses
     WHERE TRIM(text_content) = ''
       AND TRIM(content_html) != ''
       AND boost_of_uri IS NULL
     ORDER BY created_at`,
  ),
);

const statements = rows.map((row) => {
  const text = stripBasicHtmlTags(row.content_html);
  return `UPDATE remote_statuses SET text_content = ${sqlString(text)} WHERE id = ${sqlString(row.id)} AND TRIM(text_content) = ''`;
});

for (let i = 0; i < statements.length; i += batchSize) {
  executeUpdates(statements.slice(i, i + batchSize));
}

console.log(
  `${dryRun ? "planned" : "backfilled"} ${statements.length} remote_statuses.text_content row(s)`,
);
